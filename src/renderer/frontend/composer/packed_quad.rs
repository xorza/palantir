//! A quad-tier draw reduced to physical space: its rect, corners and lanes, past which the
//! composer is shape-blind.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::num::F32Px;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::renderer::frontend::composer::lowering::{Lowering, ScaledRect};
use crate::renderer::frontend::composer::template_shape::TemplateShape;
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, QuadGeometry};
use crate::renderer::quad::Quad;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::Vec2;

/// A quad-tier draw reduced to physical space: its covering rect, and the closed shape inside it
/// as sharp points, a radius at each and its inradius (`Quad::points`).
#[derive(Debug)]
pub(super) struct PackedQuad {
    /// The quad's own rect, unturned, and the whole pixels it reaches turned.
    pub(super) rect: ScaledRect,
    /// A radius at each point, `(tl, tr, br, bl)` for a box.
    pub(super) corners: Corners,
    pub(super) fill_axis: FillAxis,
    pub(super) border_width: f32,
    /// A paint animation's turn about the rect's center, in radians.
    pub(super) rotation: f32,
    /// The shape's sharp points from the rect's top-left, clockwise on screen.
    pub(super) points: [Vec2; 4],
    /// The shape's inradius (`Quad::inradius`).
    pub(super) inradius: f32,
}

impl PackedQuad {
    /// Unturned with neither a border nor a rounded corner: the edges are the rect's.
    pub(super) const fn is_sharp(&self) -> bool {
        self.rotation == 0.0 && is_invisible(self.border_width) && self.corners.is_approx_zero()
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
    // Always inlined into `quad` and `backdrop`: as a call, `repeated_chrome` ran 4 % slower.
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
                // and a backdrop's lanes are ratios.
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
                let size = rect.physical.size;
                Self {
                    rect,
                    corners,
                    fill_axis,
                    border_width: p.border.width * scale_physical,
                    rotation,
                    points: Quad::box_points(size),
                    inradius: Quad::box_inradius(size),
                }
            }
            QuadGeometry::Triangle {
                a,
                b,
                c,
                radius,
                spin,
            } => {
                let triangle = TemplateShape::triangle([a, b, c], radius, lowering);
                // Turned as a rect is, so its gradient turns with it.
                let (rect, rotation) = match spin {
                    None => (
                        ScaledRect::from_physical(triangle.rect, lowering.display.physical),
                        0.0,
                    ),
                    Some(spin) => (lowering.turned_rect(triangle.rect, spin), spin.angle),
                };
                Self {
                    rect,
                    corners: triangle.corners,
                    // A gradient axis is unit-space.
                    fill_axis: p.fill_axis,
                    border_width: (p.border.width * scale_physical).max(0.0),
                    rotation,
                    points: triangle.points,
                    inradius: triangle.inradius,
                }
            }
        }
    }
}
