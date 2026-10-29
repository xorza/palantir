//! A quad-tier draw reduced to physical space: its rect, corners and lanes, past which the
//! composer is shape-blind.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::num::F32Px;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::packed::half_simd::F16x4;
use crate::renderer::frontend::composer::lowering::{Lowering, ScaledRect};
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, QuadGeometry};
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::Vec2;

/// A quad-tier draw reduced to physical space. `corners` and `fill_axis` mean different things for
/// a rect and a triangle.
#[derive(Debug)]
pub(super) struct PackedQuad {
    /// The quad's own rect, unturned, and the whole pixels it reaches turned.
    pub(super) rect: ScaledRect,
    pub(super) corners: Corners,
    pub(super) fill_axis: FillAxis,
    pub(super) stroke_width: f32,
    /// A paint animation's turn about the rect's center, in radians.
    pub(super) rotation: f32,
}

impl PackedQuad {
    /// Unturned with neither a stroke nor a rounded corner: the edges are the rect's.
    pub(super) const fn is_sharp(&self) -> bool {
        self.rotation == 0.0 && is_invisible(self.stroke_width) && self.corners.is_approx_zero()
    }

    /// [`Self::is_sharp`] plus physical edges on whole pixels. Exactness makes the
    /// fragment fast path bitwise-identical to the SDF.
    pub(super) fn is_pixel_aligned(&self) -> bool {
        let physical = self.rect.physical;
        let max = physical.max();
        self.is_sharp()
            && physical.min.x.is_integral()
            && physical.min.y.is_integral()
            && max.x.is_integral()
            && max.y.is_integral()
    }

    /// Reduce a quad-tier draw to physical space; everything past this is shape-blind.
    // Always inlined into `quad` and `pane`: as a call, `repeated_chrome` ran 4 % slower.
    #[inline(always)]
    pub(super) fn pack(p: &DrawQuadPayload, lowering: &Lowering<'_>) -> Self {
        let scale_physical = lowering.transform.physical_scale();
        match p.geometry {
            QuadGeometry::Rect {
                rect,
                corners,
                spin,
            } => {
                // A turned rect snaps to no pixel: its edges are not on the pixel grid.
                let edges = match spin {
                    None => lowering.display.edge_rounding(),
                    Some(_) => EdgeRounding::Exact,
                };
                let source = lowering.scaled_rect_with(rect, edges);
                // Shadow scalars are logical px, so scale them; a gradient axis is unit-space,
                // and a pane's lanes are ratios.
                let fill_axis = if p.fill.kind.is_shadow() {
                    p.fill_axis.scaled_saturating(scale_physical)
                } else {
                    p.fill_axis
                };
                let corners = corners.fit_to(source.physical.size, scale_physical);
                // A drop shadow's quad is its snapped source moved and grown, not re-snapped, so
                // the source inside it lies on the same pixels as the fill.
                let rect = if p.fill.kind == FillKind::SHADOW_DROP {
                    let geometry = ShadowGeometry::from_lanes(fill_axis.lanes());
                    let physical = Rect {
                        min: source.physical.min + geometry.offset,
                        size: source.physical.size,
                    }
                    .inflated(geometry.halo());
                    ScaledRect::from_physical(physical, lowering.display.physical)
                } else {
                    source
                };
                let (rect, rotation) = match spin {
                    None => (rect, 0.0),
                    Some(spin) => (lowering.turned_rect(rect.physical, spin), spin.angle),
                };
                Self {
                    rect,
                    corners,
                    fill_axis,
                    stroke_width: p.stroke.width * scale_physical,
                    rotation,
                }
            }
            QuadGeometry::Triangle {
                a,
                b,
                c,
                radius,
                spin,
            } => {
                let [a, b, c] = match spin {
                    None => [a, b, c],
                    Some(spin) => {
                        let rotor = spin.rotor();
                        [a, b, c].map(|q| rotor.apply(q))
                    }
                };
                // No snapping: the SDF handles sub-pixel placement.
                let to_physical = lowering.transform.physical_map(Vec2::ZERO);
                let (a, b, c) = (
                    to_physical.point(a),
                    to_physical.point(b),
                    to_physical.point(c),
                );
                // Saturated as the f16 lane holds it, so the quad and the shader take one radius.
                let radius_physical = F16x4::saturate_lane((radius * scale_physical).max(0.0));
                // Covering AABB of the rounded shape; the stroke sits on the inner edge, so it adds
                // no outward reach.
                let lo = a.min(b).min(c);
                let hi = a.max(b).max(c);
                let physical_rect = Rect::from_min_max(lo, hi).inflated(radius_physical);
                // Pack the points as unorm16 shares of the covering rect and the radius as f16;
                // `FillKind::TRIANGLE` tells the shader to decode them.
                let share = |p: Vec2| {
                    let at = (p - physical_rect.min)
                        / Vec2::new(physical_rect.size.w, physical_rect.size.h);
                    [unorm16(at.x), unorm16(at.y)]
                };
                let ([ax, ay], [bx, by], [cx, cy]) = (share(a), share(b), share(c));
                let radius_f16 = F16x4::from_lanes([0.0, 0.0, radius_physical, 0.0]).lane_bits(2);
                Self {
                    rect: ScaledRect::from_physical(physical_rect, lowering.display.physical),
                    corners: Corners::from_bits([ax, ay, bx, by]),
                    fill_axis: FillAxis::from(F16x4::from_bits([cx, cy, radius_f16, 0])),
                    stroke_width: (p.stroke.width * scale_physical).max(0.0),
                    rotation: 0.0,
                }
            }
        }
    }
}

/// `v` in `0..=1` as unorm16, matching `unpack2x16unorm`: `bits / 65535`.
#[expect(
    clippy::cast_sign_loss,
    reason = "every value is clamped to a non-negative range before the cast"
)]
const fn unorm16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}
