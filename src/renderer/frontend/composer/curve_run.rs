//! A curve lowered to its instance and the sub-instances it is drawn in.

use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::lowering::Lowering;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::render_buffer::curve_caps::CurveCaps;
use crate::renderer::render_buffer::curve_instance::CurveInstance;
use crate::renderer::render_buffer::curve_kind::CurveKind;
use crate::shape::paint::curve_basis::CurveBasis;
use glam::Vec2;

/// A curve's instance in physical px, drawn as `n` sub-instances along its parameter.
#[derive(Clone, Copy, Debug)]
pub(super) struct CurveRun {
    proto: CurveInstance,
    n: u32,
}

impl CurveRun {
    /// Lower `p`, `width_physical` wide, under `lowering`: its control points or arc in physical
    /// px, and as many sub-instances as its length needs.
    pub(super) fn lower(p: &DrawCurvePayload, width_physical: f32, lowering: &Lowering<'_>) -> Self {
        let cap = p.cap;
        // Owner origin folds in here so the record stays owner-local. No snapping: it would warp
        // the traced shape.
        let to_physical = lowering.transform.physical_map(p.origin);
        let spin = p.bounds.spin();
        let proto = CurveInstance {
            width: width_physical,
            color0: p.fill.color,
            color1: p.fill.color,
            caps: CurveCaps::new(cap, true, true),
            fill_kind: p.fill.kind,
            fill_lut_row: p.fill.lut_row,
            ..bytemuck::Zeroable::zeroed()
        };
        match p.basis {
            CurveBasis::Cubic(CubicBezier { p0, p1, p2, p3 }) => {
                let mut ctrl = [p0, p1, p2, p3];
                if let Some(spin) = spin {
                    let rotor = spin.rotor();
                    for q in &mut ctrl {
                        *q = rotor.apply(*q);
                    }
                }
                let ctrl = ctrl.map(|q| to_physical.point(q));
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
                Self { proto, n }
            }
            CurveBasis::Arc(arc) => {
                let mut a1 = arc.end_angle();
                let CircularArc {
                    mut center,
                    radius,
                    start_angle: mut a0,
                    ..
                } = arc;
                if let Some(spin) = spin {
                    center = spin.rotor().apply(center);
                    a0 += spin.angle;
                    a1 += spin.angle;
                }
                // The transform is translate + uniform scale, so a circle stays a circle: transform
                // the center, scale the radius.
                let radius_physical = radius * lowering.transform.physical_scale();
                // Sub-instance count from the exact arc length; chord sagitta stays under 0.3 px
                // even at r = 1.
                let n = geometry::sub_instance_count(radius_physical * (a1 - a0).abs());
                let proto = CurveInstance {
                    p0: to_physical.point(center),
                    p1: Vec2::new(radius_physical, 0.0),
                    p2: Vec2::new(a0, a1),
                    p3: Vec2::ZERO,
                    kind: CurveKind::ARC,
                    ..proto
                };
                Self { proto, n }
            }
        }
    }

    /// Tile `t` in `[0, 1]` into `n` contiguous ranges, the last ending at exactly `1.0` so the
    /// shader's trailing-cap test fires.
    pub(super) fn push_into(self, curves: &mut Vec<CurveInstance>) {
        let Self { proto, n } = self;
        let inv_n = 1.0 / n as f32;
        for i in 0..n {
            let t1 = if i + 1 == n {
                1.0
            } else {
                (i + 1) as f32 * inv_n
            };
            curves.push(CurveInstance {
                t0: i as f32 * inv_n,
                t1,
                ..proto
            });
        }
    }
}
