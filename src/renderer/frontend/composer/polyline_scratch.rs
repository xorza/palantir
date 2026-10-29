//! A polyline lowered to its segment and joint instances, in scratch kept across frames.

use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::render_buffer::curve_caps::CurveCaps;
use crate::renderer::render_buffer::curve_instance::CurveInstance;
use crate::renderer::render_buffer::curve_kind::CurveKind;
use crate::shape::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;

/// The points of the polyline being lowered in physical px, which of them it keeps, and each kept
/// segment's direction and colors.
#[derive(Debug, Default)]
pub(super) struct PolylineScratch {
    points: Vec<Vec2>,
    kept: Vec<u32>,
    directions: Vec<Vec2>,
    /// Each kept segment's colors.
    colors: Vec<SegmentColors>,
}

/// How a kept polyline paints: its recorded colors and how they map to segments, a paint
/// animation's alpha, its physical width, its cap and its join.
#[derive(Clone, Copy, Debug)]
pub(super) struct PolylineLook<'a> {
    pub(super) colors: &'a [RgbaF16],
    pub(super) mode: ColorMode,
    pub(super) alpha: f32,
    pub(super) width: f32,
    pub(super) cap: LineCap,
    pub(super) join: LineJoin,
}

/// The two ends of one polyline segment's color ramp.
#[derive(Clone, Copy, Debug)]
struct SegmentColors {
    start: RgbaF16,
    end: RgbaF16,
}

impl SegmentColors {
    const fn flat(color: RgbaF16) -> Self {
        Self {
            start: color,
            end: color,
        }
    }
}

impl PolylineScratch {
    /// Take `points`, in physical px, and keep only those beyond the coincidence threshold from
    /// their predecessor; dropped points take their colors with them. Whether two or more stay, so
    /// the polyline paints.
    pub(super) fn keep(&mut self, points: impl Iterator<Item = Vec2>) -> bool {
        self.points.clear();
        self.points.extend(points);
        self.kept.clear();
        let mut prev: Option<Vec2> = None;
        for (i, &q) in self.points.iter().enumerate() {
            if prev.is_none_or(|p| (q - p).length_squared() > geometry::POLYLINE_COINCIDENT_EPS_SQ)
            {
                self.kept.push(i as u32);
                prev = Some(q);
            }
        }
        self.kept.len() >= 2
    }

    /// Push the kept polyline's instances into `curves`, painted as `look`: one per segment, then
    /// one per interior joint.
    pub(super) fn emit(&mut self, look: PolylineLook<'_>, curves: &mut Vec<CurveInstance>) {
        let PolylineLook {
            colors: src_colors,
            mode,
            alpha,
            width,
            cap,
            join,
        } = look;
        debug_assert!(self.kept.len() >= 2, "only a kept polyline emits");
        let Self {
            points,
            kept,
            directions,
            colors,
        } = self;
        directions.clear();
        directions.extend(
            kept.windows(2)
                .map(|pair| (points[pair[1] as usize] - points[pair[0] as usize]).normalize()),
        );
        // Each segment's colors, `k -> k+1` indexed through `kept`, worked out once: a segment's
        // and both its joints' read them. A paint animation's alpha is folded in here.
        colors.clear();
        colors.extend(kept.windows(2).map(|pair| {
            let colors = match mode {
                ColorMode::Single => SegmentColors::flat(src_colors[0]),
                ColorMode::PerPoint => SegmentColors {
                    start: src_colors[pair[0] as usize],
                    end: src_colors[pair[1] as usize],
                },
                ColorMode::PerSegment => SegmentColors::flat(src_colors[pair[1] as usize - 1]),
            };
            if alpha == 1.0 {
                colors
            } else {
                SegmentColors {
                    start: colors.start.faded(alpha),
                    end: colors.end.faded(alpha),
                }
            }
        }));
        let pts = points.as_slice();
        let kept = kept.as_slice();
        let directions = directions.as_slice();
        let colors = colors.as_slice();
        let pt = |k: usize| pts[kept[k] as usize];
        let n_segs = directions.len();
        curves.reserve(2 * n_segs - 1);
        for k in 0..n_segs {
            // Pre-oriented bisector clip planes for the joint ends; zero = cap end, no clip.
            let n_start = if k > 0 {
                -(directions[k - 1] + directions[k])
            } else {
                Vec2::ZERO
            };
            let n_end = if k + 1 < n_segs {
                directions[k] + directions[k + 1]
            } else {
                Vec2::ZERO
            };
            let segment = colors[k];
            curves.push(CurveInstance {
                p0: pt(k),
                p1: n_start,
                p2: n_end,
                p3: pt(k + 1),
                t0: 0.0,
                t1: 1.0,
                width,
                color0: segment.start,
                color1: segment.end,
                caps: CurveCaps::new(cap, k == 0, k + 1 == n_segs),
                kind: CurveKind::SEGMENT,
                fill_kind: FillKind::SOLID,
                ..bytemuck::Zeroable::zeroed()
            });
        }
        // One chrome instance per interior joint fills the wedge between segment end
        // faces, in the premultiplied linear average of the adjacent colors.
        for k in 1..n_segs {
            let d_a = directions[k - 1];
            let d_b = directions[k];
            let ca = colors[k - 1].end;
            let cb = colors[k].start;
            let color = if ca == cb {
                ca
            } else {
                premultiplied_midpoint(ca.into(), cb.into()).to_f16()
            };
            curves.push(CurveInstance {
                p0: pt(k),
                p1: -d_a,
                p2: d_b,
                t0: 0.0,
                t1: 1.0,
                width,
                color0: color,
                color1: color,
                kind: geometry::polyline_join_kind(d_a, d_b, join),
                fill_kind: FillKind::SOLID,
                ..bytemuck::Zeroable::zeroed()
            });
        }
    }
}

/// The straight color halfway between `a` and `b`, interpolated premultiplied. A midpoint with no
/// alpha is transparent black.
fn premultiplied_midpoint(a: RgbaF32, b: RgbaF32) -> RgbaF32 {
    let (a, b) = (a.premultiplied(), b.premultiplied());
    let alpha = f32::midpoint(a.a, b.a);
    if alpha <= 0.0 {
        return RgbaF32::TRANSPARENT;
    }
    let channel = |x: f32, y: f32| f32::midpoint(x, y) / alpha;
    RgbaF32 {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: alpha,
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::polyline_scratch::PolylineScratch;
    use glam::Vec2;

    impl PolylineScratch {
        /// Each kept segment's unit direction, from the last polyline lowered.
        pub(crate) fn directions(&self) -> &[Vec2] {
            &self.directions
        }

        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                points,
                kept,
                directions,
                colors,
            } = self;
            scratch_report::vec_bytes(points)
                + scratch_report::vec_bytes(kept)
                + scratch_report::vec_bytes(directions)
                + scratch_report::vec_bytes(colors)
        }
    }
}
