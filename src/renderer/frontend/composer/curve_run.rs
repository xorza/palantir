//! A curve lowered to its instance and the sub-instances it is drawn in.
//!
//! - **A dash is cut on the CPU, at lowering.** Each dash is its own run of sub-instances over a
//!   range of the curve's parameter, found by arc length, and ends in the stroke's cap. The
//!   shader draws it as it draws any sub-range, so a dashed curve costs an instance for each
//!   dash and no shader work. As Skia's dash path effect and SVG's `stroke-dasharray` cut a path
//!   before it is stroked. Refused: the dash in the fragment shader, which needs the arc length
//!   at each fragment and its own antialiasing at each dash end, where a cap has both.

use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::paint::line_style::{Dash, LineCap};
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::lowering::Lowering;
use crate::renderer::frontend::payload::draw_stroke_payload::DrawStrokePayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::curve_caps::{CapEnds, CurveCaps};
use crate::renderer::render_buffer::curve_instance::CurveInstance;
use crate::renderer::render_buffer::curve_kind::CurveKind;
use crate::shape::geometry::curve_basis::CurveBasis;
use glam::Vec2;

/// A curve's instance in physical px, drawn as `n` sub-instances along its parameter, or as
/// its dashes.
#[derive(Clone, Copy, Debug)]
pub(super) struct CurveRun {
    proto: CurveInstance,
    n: u32,
    cap: LineCap,
    /// Where the dashes fall, for a curve that breaks into them.
    dash: Option<DashCut>,
}

/// A dash pattern in physical px and the arc length of the curve it breaks.
#[derive(Clone, Copy, Debug)]
struct DashCut {
    dash: Dash,
    length: ArcLength,
}

/// The most dashes one curve breaks into: past it the pattern stretches to fit, so a curve
/// zoomed far in costs a bounded count of instances.
const MAX_DASHES: u32 = 4096;

/// The chords a cubic's arc length is read off: its parameter is not its length, so a dash's
/// ends are found on the polyline through this many equal steps of `t`.
const ARC_SAMPLES: usize = 64;

/// A curve's arc length against its parameter.
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "a value on the stack of one curve's lowering, never held in a collection"
)]
enum ArcLength {
    /// A parameter that is the length to scale, an arc's: `t = s / total`.
    Uniform { total: f32 },
    /// A cubic's: the running length of its chords at equal steps of `t`, read both ways by
    /// linear interpolation. `cum[i]` is the length from `t = 0` to `t = i / ARC_SAMPLES`.
    Sampled { cum: [f32; ARC_SAMPLES + 1] },
}

impl ArcLength {
    fn of_cubic(bezier: CubicBezier) -> Self {
        let CubicBezier { p0, p1, p2, p3 } = bezier;
        let mut cum = [0.0; ARC_SAMPLES + 1];
        let mut prev = p0;
        for i in 1..=ARC_SAMPLES {
            let t = i as f32 / ARC_SAMPLES as f32;
            let u = 1.0 - t;
            let at = (u * u * u) * p0
                + (3.0 * u * u * t) * p1
                + (3.0 * u * t * t) * p2
                + (t * t * t) * p3;
            cum[i] = cum[i - 1] + (at - prev).length();
            prev = at;
        }
        Self::Sampled { cum }
    }

    const fn total(&self) -> f32 {
        match self {
            Self::Uniform { total } => *total,
            Self::Sampled { cum } => cum[ARC_SAMPLES],
        }
    }

    /// The parameter at length `s` along the curve, `s` held within it.
    fn t_at(&self, s: f32) -> f32 {
        let s = s.clamp(0.0, self.total());
        match self {
            Self::Uniform { total } => {
                if *total > 0.0 {
                    s / total
                } else {
                    0.0
                }
            }
            Self::Sampled { cum } => {
                // The first sample at or past `s`; the chord before it holds `s`.
                let hi = cum.partition_point(|&len| len < s).clamp(1, ARC_SAMPLES);
                let (a, b) = (cum[hi - 1], cum[hi]);
                let within = if b > a { (s - a) / (b - a) } else { 0.0 };
                ((hi - 1) as f32 + within) / ARC_SAMPLES as f32
            }
        }
    }
}

impl CurveRun {
    /// `basis` turned by `spin`, its pivot relative to the payload's origin: a cubic's control
    /// points turned, as a Bézier is affine-invariant, an arc's center turned and its angles
    /// shifted, as a circle stays one. The composer culls the turned basis by its bounds at this
    /// frame's angle.
    pub(super) fn turned(basis: CurveBasis, spin: Spin) -> CurveBasis {
        let rotor = spin.rotor();
        match basis {
            CurveBasis::Cubic(CubicBezier { p0, p1, p2, p3 }) => {
                let [p0, p1, p2, p3] = [p0, p1, p2, p3].map(|q| rotor.apply(q));
                CurveBasis::Cubic(CubicBezier { p0, p1, p2, p3 })
            }
            CurveBasis::Arc(arc) => CurveBasis::Arc(CircularArc {
                center: rotor.apply(arc.center),
                start_angle: arc.start_angle + spin.angle,
                ..arc
            }),
        }
    }

    /// Lower `basis`, `p`'s curve turned already ([`Self::turned`]), under `lowering`: its control
    /// points or arc in physical px, as many sub-instances as its length needs, and where its
    /// dashes fall in physical px.
    pub(super) fn lower(basis: CurveBasis, p: &DrawStrokePayload, lowering: &Lowering<'_>) -> Self {
        let cap = p.cap;
        let scale = lowering.transform.physical_scale();
        if let Some(dash) = p.dash {
            debug_assert!(!dash.is_solid(), "the stroke's lowering drops a solid dash");
        }
        let dash = p
            .dash
            .map(|dash| Dash::new(dash.on * scale, dash.off * scale));
        // Owner origin folds in here so the record stays owner-local. No snapping: it would warp
        // the traced shape.
        let to_physical = lowering.transform.physical_map(p.origin);
        let proto = CurveInstance {
            width: p.width * scale,
            color0: p.fill.color,
            color1: p.fill.color,
            fill_kind: p.fill.kind,
            fill_lut_row: p.fill.lut_row,
            ..bytemuck::Zeroable::zeroed()
        };
        match basis {
            CurveBasis::Cubic(CubicBezier { p0, p1, p2, p3 }) => {
                let ctrl = [p0, p1, p2, p3].map(|q| to_physical.point(q));
                let [p0, p1, p2, p3] = ctrl;
                // Adaptive sub-instance count from the control-polygon length, which bounds arc
                // length from above (no faceting). Near-straight cubics use one instance: every
                // chord of a flat curve lies on the segment.
                let n = if geometry::cubic_is_flat(ctrl) {
                    1
                } else {
                    let l = (p1 - p0).length() + (p2 - p1).length() + (p3 - p2).length();
                    geometry::sub_instance_count(l)
                };
                let proto = CurveInstance {
                    p0,
                    p1,
                    p2,
                    p3,
                    kind: CurveKind::CUBIC,
                    ..proto
                };
                let dash = dash.map(|dash| DashCut {
                    dash,
                    length: ArcLength::of_cubic(CubicBezier { p0, p1, p2, p3 }),
                });
                Self {
                    proto,
                    n,
                    cap,
                    dash,
                }
            }
            CurveBasis::Arc(arc) => {
                let a1 = arc.end_angle();
                let CircularArc {
                    center,
                    radius,
                    start_angle: a0,
                    ..
                } = arc;
                // The transform is translate + uniform scale, so a circle stays a circle: transform
                // the center, scale the radius.
                let radius_physical = radius * scale;
                let length = radius_physical * (a1 - a0).abs();
                // Sub-instance count from the exact arc length; chord sagitta stays under 0.3 px
                // even at r = 1.
                let n = geometry::sub_instance_count(length);
                let proto = CurveInstance {
                    p0: to_physical.point(center),
                    p1: Vec2::new(radius_physical, 0.0),
                    p2: Vec2::new(a0, a1),
                    p3: Vec2::ZERO,
                    kind: CurveKind::ARC,
                    ..proto
                };
                let dash = dash.map(|dash| DashCut {
                    dash,
                    length: ArcLength::Uniform { total: length },
                });
                Self {
                    proto,
                    n,
                    cap,
                    dash,
                }
            }
        }
    }

    /// Push the curve's sub-instances, each cut by the rounded clips of `clip`: the whole curve
    /// as `n` contiguous ranges of `t`, or one run for each dash of its pattern.
    pub(super) fn push_into(self, curves: &mut Vec<CurveInstance>, clip: ClipChain) {
        let Self {
            proto,
            n,
            cap,
            dash,
        } = self;
        let stroke = Stroked {
            proto: CurveInstance { clip, ..proto },
            cap,
        };
        match dash {
            None => stroke.push_range(0.0, 1.0, n, curves),
            Some(cut) => stroke.push_dashes(&cut, curves),
        }
    }
}

/// A curve's instance with its clip set, and the cap each stroke cut from it ends in.
#[derive(Clone, Copy, Debug)]
struct Stroked {
    proto: CurveInstance,
    cap: LineCap,
}

impl Stroked {
    /// Tile `[t0, t1]` of the curve into `n` contiguous ranges, the last ending at exactly `t1`.
    /// The first starts the stroke the range is and the last ends it, so each takes the cap on
    /// that end alone.
    fn push_range(self, t0: f32, t1: f32, n: u32, curves: &mut Vec<CurveInstance>) {
        let Self { proto, cap } = self;
        let step = (t1 - t0) / n as f32;
        for i in 0..n {
            let end = if i + 1 == n {
                t1
            } else {
                t0 + (i + 1) as f32 * step
            };
            curves.push(CurveInstance {
                t0: t0 + i as f32 * step,
                t1: end,
                caps: CurveCaps::new(cap, CapEnds::of_segment(i as usize, n as usize)),
                ..proto
            });
        }
    }

    /// Break the curve into `dash`'s pattern from its start: each dash a run of its own over
    /// the parameter range its arc length covers. A pattern of more than [`MAX_DASHES`] dashes
    /// stretches to that many.
    fn push_dashes(self, cut: &DashCut, curves: &mut Vec<CurveInstance>) {
        let DashCut { dash, length: arc } = cut;
        let total = arc.total();
        let stretch = (total / (dash.period() * MAX_DASHES as f32)).max(1.0);
        let (on, period) = (dash.on * stretch, dash.period() * stretch);
        // A curve of no length is one dash of none, which draws its caps as the whole curve
        // would.
        #[expect(
            clippy::cast_sign_loss,
            reason = "a length over a length is not negative"
        )]
        let count = ((total / period).ceil() as u32).clamp(1, MAX_DASHES);
        for k in 0..count {
            let start = k as f32 * period;
            let end = (start + on).min(total);
            self.push_range(
                arc.t_at(start),
                arc.t_at(end),
                geometry::sub_instance_count(end - start),
                curves,
            );
        }
    }
}
