use crate::primitives::approx::noop_f32;
use crate::primitives::color::Color;
use crate::primitives::mesh::MeshVertex;
use crate::shape::{ColorMode, LineCap, LineJoin};
use glam::Vec2;

const HALF_FRINGE: f32 = 0.5;
/// SVG default. Beyond this the miter would project a long spike,
/// so we fall back to bevel geometry at the join instead.
const MITER_LIMIT: f32 = 4.0;
const MIN_ROUND_FAN_SEGS: u16 = 4;
const MAX_ROUND_FAN_SEGS: u16 = 16;
/// Threshold on `(normal_prev + normal_next).length_squared()`
/// below which the two normals count as antiparallel (180° fold).
const ANTIPARALLEL_EPS_SQ: f32 = 1e-6;
/// Threshold on segment length squared below which two consecutive
/// points count as coincident — the emit walker skips them so the
/// degenerate segment contributes no geometry.
const COINCIDENT_EPS_SQ: f32 = 1e-12;

#[derive(Clone, Copy)]
pub(crate) struct StrokeStyle {
    pub(crate) mode: ColorMode,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    pub(crate) width_phys: f32,
}

/// Tessellate a stroked polyline as a fringe-AA mesh.
///
/// Inputs are in **physical px** — composer applies the active
/// transform + DPI scale to `points` and `width_phys` before
/// calling. Colors are premultiplied linear RGBA. `mode` picks
/// the color-storage interpretation and the vertex layout:
///
/// - [`ColorMode::Single`] — `colors.len() == 1`. Same color on
///   every cross-section.
/// - [`ColorMode::PerPoint`] — `colors.len() == points.len()`. GPU
///   lerps between adjacent cross-sections, giving a smooth
///   gradient along the stroke.
/// - [`ColorMode::PerSegment`] — `colors.len() == points.len() - 1`.
///   Each segment paints as a solid block; interior cross-sections
///   duplicate so colors don't bleed at joins. Join chrome (bevel
///   bridge / round fan / concave fill) and round caps paint with
///   the average of the two adjacent segments' colors.
///
/// **Hairline behavior.** For `width_phys < 1`, geometry freezes
/// at 1 physical px wide and per-vertex colors are alpha-scaled by
/// `width_phys` (premultiplied → rgb and alpha by the same
/// factor). A 0.3-px line paints as a 1-px line at α=0.3 of each
/// vertex's input color.
///
/// **Joins.** Miter clamped to [`MITER_LIMIT`] (falls back to bevel
/// geometry past the limit); Bevel and Round selectable per stroke.
/// **Caps.** Butt, Square, and Round selectable per stroke.
///
/// **Indexing.** Indices are pushed **0-based** to the verts this
/// call emits — composer captures `phys_v_start = out_verts.len()`
/// before calling and passes it as `MeshDraw.vertices.start`,
/// which becomes the wgpu `base_vertex`. Multiple calls into the
/// same vecs concatenate independent index blocks.
///
/// **Degenerate input.** Consecutive coincident points
/// (`(p[i+1] - p[i]).length_squared() <= 1e-12`) are skipped on
/// the fly — the corresponding zero-length segment contributes no
/// geometry and its color (PerPoint / PerSegment) is dropped. The
/// rest of the polyline tessellates as if those points weren't
/// there. A polyline that collapses to fewer than two distinct
/// points emits nothing.
///
/// **Index width.** Indices are `u16` and scoped per-call:
/// emitting more than 65 535 verts in a single call panics.
/// Composer is expected to split when needed.
pub(crate) fn tessellate_polyline_aa(
    points: &[Vec2],
    colors: &[Color],
    style: StrokeStyle,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    if points.len() < 2 {
        return;
    }
    // Reject NaN, zero, near-zero, and negative widths up front —
    // they'd produce invisible verts at best and NaN positions at
    // worst. `noop_f32` is the shared "non-paintable scalar"
    // predicate used elsewhere for Stroke/Color/Shape.
    if noop_f32(style.width_phys) {
        return;
    }
    assert!(matches_mode(points.len(), colors.len(), style.mode));

    let half_geom = (style.width_phys * 0.5).max(HALF_FRINGE);
    let geo = Geo {
        outer_offset: half_geom + HALF_FRINGE,
        inner_offset: half_geom,
        alpha_scale: style.width_phys.clamp(0.0, 1.0),
        cap: style.cap,
        join: style.join,
    };
    let mut e = Emitter {
        call_start: out_verts.len(),
        verts: out_verts,
        indices: out_indices,
        geo,
    };
    emit_polyline(points, ColorPlan::from_mode(style.mode, colors), &mut e);
}

/// Per-kept-point color picker. Pre-resolved from `(ColorMode,
/// colors)` so the emit walker doesn't carry the slice + mode
/// around separately.
///
/// At every kept point the walker asks for the `(trailing,
/// leading)` pair: the colors for the segments incoming to and
/// outgoing from that point. At endpoints one side doesn't exist,
/// so both sides return the same color (which the walker uses for
/// the cap). For PerPoint/Single, trailing and leading are always
/// equal; for PerSegment they differ at color boundaries.
#[derive(Clone, Copy)]
enum ColorPlan<'a> {
    Single(Color),
    PerPoint(&'a [Color]),
    PerSegment(&'a [Color]),
}

impl<'a> ColorPlan<'a> {
    fn from_mode(mode: ColorMode, colors: &'a [Color]) -> Self {
        match mode {
            ColorMode::Single => ColorPlan::Single(colors[0]),
            ColorMode::PerPoint => ColorPlan::PerPoint(colors),
            ColorMode::PerSegment => ColorPlan::PerSegment(colors),
        }
    }

    /// `(trailing, leading)` at kept point with original index `i`.
    /// `is_first` / `is_last` signal endpoints — there the missing
    /// side mirrors the present side so the walker has a single
    /// cap color to use.
    fn at(self, i: usize, is_first: bool, is_last: bool) -> (Color, Color) {
        match self {
            ColorPlan::Single(c) => (c, c),
            ColorPlan::PerPoint(cs) => (cs[i], cs[i]),
            ColorPlan::PerSegment(cs) => {
                let trailing = if is_first { cs[0] } else { cs[i - 1] };
                let leading = if is_last { trailing } else { cs[i] };
                (trailing, leading)
            }
        }
    }
}

/// Geometry + style parameters shared by both emit paths. Pre-
/// computed in [`tessellate_polyline_aa`]'s setup so the inner
/// loops just read the resolved values.
#[derive(Clone, Copy)]
struct Geo {
    outer_offset: f32,
    inner_offset: f32,
    alpha_scale: f32,
    cap: LineCap,
    join: LineJoin,
}

impl Geo {
    /// Cap-extension distance along the segment direction at
    /// endpoints. Only `LineCap::Square` extends; Butt and Round
    /// leave the cross-section at the endpoint and use their
    /// fan-emission paths for visible chrome.
    #[inline]
    fn cap_extension(&self) -> f32 {
        match self.cap {
            LineCap::Square => self.inner_offset,
            LineCap::Butt | LineCap::Round => 0.0,
        }
    }
}

/// Resolved geometry for an interior join, combining the
/// sharp-vs-smooth classification with the line-join policy.
/// `Dual` carries the two segment normals (sharp joins, or smooth
/// joins under Bevel/Round); `Single` carries the miter bisector
/// and extension factor (smooth join under Miter).
enum InteriorJoin {
    Dual {
        normal_prev: Vec2,
        normal_next: Vec2,
    },
    Single {
        bisector: Vec2,
        ext: f32,
    },
}

#[inline]
fn resolve_interior_join(normal_prev: Vec2, normal_next: Vec2, join: LineJoin) -> InteriorJoin {
    let sum = normal_prev + normal_next;
    let len_sq = sum.length_squared();
    if len_sq < ANTIPARALLEL_EPS_SQ {
        return InteriorJoin::Dual {
            normal_prev,
            normal_next,
        };
    }
    let bisector = sum / len_sq.sqrt();
    let cos_half = bisector.dot(normal_prev);
    let sharp = cos_half < 1.0 / MITER_LIMIT;
    let dual = sharp || !matches!(join, LineJoin::Miter);
    if dual {
        InteriorJoin::Dual {
            normal_prev,
            normal_next,
        }
    } else {
        InteriorJoin::Single {
            bisector,
            ext: 1.0 / cos_half,
        }
    }
}

/// Unified emit walker for all three color modes. Iterates kept
/// points, dispatches on `(prev_seg_normal, next_seg_normal)` to
/// pick start cap / interior join / end cap, and at every interior
/// join asks [`resolve_interior_join`] for the geometry. The
/// per-mode behavior is isolated to [`ColorPlan`].
///
/// `prev_block_offset` is the leading cross-section offset of the
/// previous kept point — i.e. the block to use as the start of
/// the next strip. Single emit point per kept index keeps the
/// strip math the same regardless of how many cross-sections were
/// pushed at the previous point (1 for merged smooth-miter, 2 for
/// dual joins or color boundaries).
fn emit_polyline(points: &[Vec2], plan: ColorPlan, e: &mut Emitter) {
    let n = points.len();
    let second = next_kept(points, 0);
    // Fewer than two distinct points — nothing to draw.
    if second >= n {
        return;
    }

    let mut prev_block_offset: u16 = 0;
    let mut prev_seg_normal: Option<Vec2> = None;
    let mut i = 0;

    while i < n {
        let next_idx = next_kept(points, i);
        let next_seg_normal = if next_idx < n {
            Some(seg_normal(points[i], points[next_idx]))
        } else {
            None
        };

        let is_first = prev_seg_normal.is_none();
        let is_last = next_seg_normal.is_none();
        let (trailing_color, leading_color) = plan.at(i, is_first, is_last);
        let trailing_color = trailing_color.scale_premultiplied(e.geo.alpha_scale);
        let leading_color = leading_color.scale_premultiplied(e.geo.alpha_scale);
        let current_offset = e.cursor();

        match (prev_seg_normal, next_seg_normal) {
            (None, Some(nn)) => {
                // Start cap. `leading_color` carries the cap color
                // (trailing == leading at endpoints per ColorPlan).
                let p = points[i] - tangent_of(nn) * e.geo.cap_extension();
                e.push_cross_section(p, nn, 1.0, leading_color);
                if matches!(e.geo.cap, LineCap::Round) {
                    e.push_round_cap(points[i], -tangent_of(nn), leading_color);
                }
                prev_block_offset = current_offset;
            }
            (Some(np), None) => {
                // End cap.
                let p = points[i] + tangent_of(np) * e.geo.cap_extension();
                e.push_cross_section(p, np, 1.0, trailing_color);
                e.push_strip_indices(prev_block_offset, current_offset);
                if matches!(e.geo.cap, LineCap::Round) {
                    e.push_round_cap(points[i], tangent_of(np), trailing_color);
                }
            }
            (Some(np), Some(nn)) => match resolve_interior_join(np, nn, e.geo.join) {
                InteriorJoin::Dual {
                    normal_prev,
                    normal_next,
                } => {
                    e.push_cross_section(points[i], normal_prev, 1.0, trailing_color);
                    e.push_strip_indices(prev_block_offset, current_offset);
                    let leading_offset = e.cursor();
                    e.push_cross_section(points[i], normal_next, 1.0, leading_color);
                    e.push_join_chrome(
                        points[i],
                        current_offset,
                        leading_offset,
                        normal_prev,
                        normal_next,
                        trailing_color.midpoint(leading_color),
                    );
                    prev_block_offset = leading_offset;
                }
                InteriorJoin::Single { bisector, ext } if trailing_color == leading_color => {
                    // Same color + smooth miter ⇒ one cross-section
                    // serves both segments. For Single/PerPoint this
                    // branch always fires (trailing == leading).
                    e.push_cross_section(points[i], bisector, ext, trailing_color);
                    e.push_strip_indices(prev_block_offset, current_offset);
                    prev_block_offset = current_offset;
                }
                InteriorJoin::Single { bisector, ext } => {
                    // Smooth miter with a color boundary (PerSegment
                    // at a color change). Two cross-sections at the
                    // same direction with different colors.
                    e.push_cross_section(points[i], bisector, ext, trailing_color);
                    e.push_strip_indices(prev_block_offset, current_offset);
                    let leading_offset = e.cursor();
                    e.push_cross_section(points[i], bisector, ext, leading_color);
                    prev_block_offset = leading_offset;
                }
            },
            (None, None) => unreachable!("guarded by early return"),
        }

        prev_seg_normal = next_seg_normal;
        i = next_idx;
    }
}

/// Mutable cursor over the output vert + index vecs plus the
/// resolved `Geo`. All chrome / cross-section helpers hang off
/// this so the per-call vertex base, output vecs, and geometry
/// parameters are threaded through one self reference instead
/// of six `&mut` arguments per call.
struct Emitter<'a> {
    verts: &'a mut Vec<MeshVertex>,
    indices: &'a mut Vec<u16>,
    call_start: usize,
    geo: Geo,
}

impl<'a> Emitter<'a> {
    /// Per-call vertex offset as `u16`. Panics if this call has
    /// emitted more than `u16::MAX` verts — see the doc on
    /// [`tessellate_polyline_aa`].
    #[inline]
    fn cursor(&self) -> u16 {
        u16::try_from(self.verts.len() - self.call_start)
            .expect("polyline tessellation exceeded u16 vertex limit")
    }

    #[inline]
    fn push_vert(&mut self, pos: Vec2, color: Color) {
        self.verts.push(MeshVertex { pos, color });
    }

    #[inline]
    fn push_cross_section(&mut self, p: Vec2, normal: Vec2, ext: f32, inner_color: Color) {
        let outer = normal * (self.geo.outer_offset * ext);
        let inner = normal * (self.geo.inner_offset * ext);
        self.verts.extend_from_slice(&[
            MeshVertex {
                pos: p + outer,
                color: Color::TRANSPARENT,
            },
            MeshVertex {
                pos: p + inner,
                color: inner_color,
            },
            MeshVertex {
                pos: p - inner,
                color: inner_color,
            },
            MeshVertex {
                pos: p - outer,
                color: Color::TRANSPARENT,
            },
        ]);
    }

    /// Three quads per segment: outer-left fringe, full-α core,
    /// outer-right fringe. `a` and `b` are u16 vert offsets to
    /// the two cross-section blocks bracketing the segment.
    #[inline]
    fn push_strip_indices(&mut self, a: u16, b: u16) {
        let a1 = a + 1;
        let a2 = a + 2;
        let a3 = a + 3;
        let b1 = b + 1;
        let b2 = b + 2;
        let b3 = b + 3;
        self.indices.extend_from_slice(&[
            a, a1, b1, a, b1, b, a1, a2, b2, a1, b2, b1, a2, a3, b3, a2, b3, b2,
        ]);
    }

    /// Dispatch to bevel / round chrome plus the concave-side
    /// notch fill. Shared between [`emit_simple`] and
    /// [`emit_per_segment`] so the two paths can't drift on join
    /// geometry.
    fn push_join_chrome(
        &mut self,
        center: Vec2,
        trailing_block: u16,
        leading_block: u16,
        normal_prev: Vec2,
        normal_next: Vec2,
        inner_color: Color,
    ) {
        match self.geo.join {
            LineJoin::Round => self.push_round_join(center, normal_prev, normal_next, inner_color),
            LineJoin::Bevel | LineJoin::Miter => self.push_bevel_bridge(
                center,
                trailing_block,
                leading_block,
                normal_prev,
                normal_next,
                inner_color,
            ),
        }
        self.push_concave_fill(
            center,
            trailing_block,
            leading_block,
            normal_prev,
            normal_next,
            inner_color,
        );
    }

    /// Bridge the convex-side gap at a beveled join. The cross
    /// product of the two normals picks the convex side:
    /// positive → CCW turn → convex on `-normal` side (verts
    /// 2,3); negative → CW turn → convex on `+normal` side
    /// (verts 0,1). Emits a center triangle at `P` plus one
    /// fringe quad joining the inner-edge + outer-fringe verts on
    /// the convex side.
    fn push_bevel_bridge(
        &mut self,
        center: Vec2,
        trailing_block: u16,
        leading_block: u16,
        normal_prev: Vec2,
        normal_next: Vec2,
        inner_color: Color,
    ) {
        let cross = normal_prev.perp_dot(normal_next);
        let (inner_off, outer_off) = if cross > 0.0 { (2, 3) } else { (1, 0) };
        let t_inner = trailing_block + inner_off;
        let t_outer = trailing_block + outer_off;
        let l_inner = leading_block + inner_off;
        let l_outer = leading_block + outer_off;
        // Center vert closes the wedge between corner point P and
        // the bridge's inner edge — without it the strip end-edges
        // leave a pinhole at P.
        let center_idx = self.cursor();
        self.push_vert(center, inner_color);
        self.indices
            .extend_from_slice(&[center_idx, t_inner, l_inner]);
        self.indices
            .extend_from_slice(&[t_inner, t_outer, l_outer, t_inner, l_outer, l_inner]);
    }

    /// Concave-side fill at a dual join. The two adjacent strips
    /// terminate their concave-inner edges at different positions
    /// (each perpendicular to its own segment), leaving a notch
    /// on the inside of the corner. Close it with a triangle
    /// anchored at `P` plus the two concave inner verts. The
    /// outer-fringe gap stays uncovered (AA gradient → invisible
    /// at typical zoom).
    fn push_concave_fill(
        &mut self,
        center: Vec2,
        trailing_block: u16,
        leading_block: u16,
        normal_prev: Vec2,
        normal_next: Vec2,
        inner_color: Color,
    ) {
        let cross = normal_prev.perp_dot(normal_next);
        let inner_off: u16 = if cross > 0.0 { 1 } else { 2 };
        let t_concave = trailing_block + inner_off;
        let l_concave = leading_block + inner_off;
        let center_idx = self.cursor();
        self.push_vert(center, inner_color);
        self.indices
            .extend_from_slice(&[center_idx, t_concave, l_concave]);
    }

    /// Round-cap fan: half-disc centered at `center`, opening
    /// toward `outward`.
    fn push_round_cap(&mut self, center: Vec2, outward: Vec2, inner_color: Color) {
        let n = round_segments(self.geo.inner_offset);
        self.push_round_fan(center, outward, std::f32::consts::FRAC_PI_2, n, inner_color);
    }

    /// Round join: arc fan filling the convex-side wedge between
    /// the two segments.
    fn push_round_join(
        &mut self,
        center: Vec2,
        normal_prev: Vec2,
        normal_next: Vec2,
        inner_color: Color,
    ) {
        let cross = normal_prev.perp_dot(normal_next);
        let (convex_prev, convex_next) = if cross > 0.0 {
            (-normal_prev, -normal_next)
        } else {
            (normal_prev, normal_next)
        };
        let sum = convex_prev + convex_next;
        let sum_len_sq = sum.length_squared();
        // Antiparallel (180° fold) → bisector degenerate; pick a
        // perpendicular and use a full half-disc.
        let (bisector, half_angle) = if sum_len_sq < ANTIPARALLEL_EPS_SQ {
            (
                Vec2::new(-convex_prev.y, convex_prev.x),
                std::f32::consts::FRAC_PI_2,
            )
        } else {
            let bisector = sum / sum_len_sq.sqrt();
            let cos_full = convex_prev.dot(convex_next).clamp(-1.0, 1.0);
            (bisector, cos_full.acos() * 0.5)
        };
        let n = round_segments(self.geo.inner_offset);
        self.push_round_fan(center, bisector, half_angle, n, inner_color);
    }

    /// Emit an arc fan centered at `center`, opening toward
    /// `center_dir`, sweeping `±half_angle`. Pushes 1 center vert
    /// plus 2·(`segments`+1) arc verts (alternating inner / outer
    /// fringe).
    fn push_round_fan(
        &mut self,
        center: Vec2,
        center_dir: Vec2,
        half_angle: f32,
        segments: u16,
        inner_color: Color,
    ) {
        let n = segments.max(1);
        let step = 2.0 * half_angle / n as f32;
        let start_angle = -half_angle;
        let perp = Vec2::new(-center_dir.y, center_dir.x);
        let base = self.cursor();
        self.push_vert(center, inner_color);
        for k in 0..=n {
            let angle = start_angle + k as f32 * step;
            let (s, c) = angle.sin_cos();
            let dir = c * center_dir + s * perp;
            self.push_vert(center + dir * self.geo.inner_offset, inner_color);
            self.push_vert(center + dir * self.geo.outer_offset, Color::TRANSPARENT);
        }
        for k in 0..n {
            let inner_k = base + 1 + 2 * k;
            let outer_k = base + 2 + 2 * k;
            let inner_k1 = base + 1 + 2 * (k + 1);
            let outer_k1 = base + 2 + 2 * (k + 1);
            self.indices.extend_from_slice(&[base, inner_k, inner_k1]);
            self.indices
                .extend_from_slice(&[inner_k, outer_k, outer_k1]);
            self.indices
                .extend_from_slice(&[inner_k, outer_k1, inner_k1]);
        }
    }
}

fn matches_mode(points_len: usize, colors_len: usize, mode: ColorMode) -> bool {
    match mode {
        ColorMode::Single => colors_len == 1,
        ColorMode::PerPoint => colors_len == points_len,
        ColorMode::PerSegment => colors_len + 1 == points_len,
    }
}

/// Number of fan slices for a round cap or join. Scales with the
/// stroke's geometry-half so a 1 px hairline cap is the cheap
/// minimum and a fat stroke gets a smooth arc.
#[inline]
fn round_segments(inner_offset: f32) -> u16 {
    (inner_offset.ceil() as u16 * 2).clamp(MIN_ROUND_FAN_SEGS, MAX_ROUND_FAN_SEGS)
}

#[inline]
fn tangent_of(normal: Vec2) -> Vec2 {
    Vec2::new(normal.y, -normal.x)
}

#[inline]
fn seg_normal(a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let len_sq = d.length_squared();
    // Internal invariant: emit_simple / emit_per_segment route
    // through next_kept so all (a, b) pairs reaching here are
    // non-coincident. Release-assert as defense against logic bugs.
    assert!(
        len_sq > COINCIDENT_EPS_SQ,
        "stroke_tessellate: seg_normal called on coincident points — emit walker bug"
    );
    let d = d / len_sq.sqrt();
    Vec2::new(-d.y, d.x)
}

/// Index of the next point in `points` whose distance from
/// `points[i]` exceeds the coincidence threshold, or
/// `points.len()` if no such point exists. Coincident points
/// in between are skipped — they contribute no geometry.
#[inline]
fn next_kept(points: &[Vec2], i: usize) -> usize {
    let mut j = i + 1;
    while j < points.len() && (points[j] - points[i]).length_squared() <= COINCIDENT_EPS_SQ {
        j += 1;
    }
    j
}

#[cfg(test)]
mod tests;
