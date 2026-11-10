//! A closed draw reduced to physical space: its rect, corners and the shape the template takes,
//! past which the composer is shape-blind.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::num::F32Px;
use crate::renderer::frontend::composer::lowering::{Lowering, ScaledRect};
use crate::renderer::frontend::composer::template_shape::TemplateShape;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::quad::Quad;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::Vec2;

/// A closed draw reduced to physical space: its covering rect, and the closed shape inside it as
/// sharp points, a radius at each and its inradius (`Quad::points`).
#[derive(Debug)]
pub(super) struct PackedQuad {
    /// The quad's own rect, unturned, and the whole pixels it reaches turned.
    pub(super) rect: ScaledRect,
    /// A radius at each point, `(tl, tr, br, bl)` for a box.
    pub(super) corners: Corners,
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

    /// Reduce `p`'s shape, under `border`, to physical space; everything past this is
    /// shape-blind.
    // Always inlined into `fill` and `effect`: as a call, `repeated_chrome` ran 4 % slower.
    #[inline(always)]
    pub(super) fn pack<P>(
        p: &DrawClosedPayload<P>,
        border: LoweredBorder,
        lowering: &Lowering<'_>,
    ) -> Self {
        let scale_physical = lowering.transform.physical_scale();
        let unturned = match p.geometry {
            PlacedClosed::Box {
                placed: PlacedBox { rect, corners },
                ..
            } => {
                let rect = Self::snapped(rect, p.spin, lowering);
                let corners = corners.fit_to(rect.physical.size, scale_physical);
                Self::of_box(rect, corners, border.width * scale_physical)
            }
            PlacedClosed::Triangle(placed) => {
                let triangle = TemplateShape::triangle(placed.centers, placed.radius, lowering);
                Self {
                    rect: ScaledRect::from_physical(triangle.rect, lowering.display.physical),
                    corners: triangle.corners,
                    border_width: (border.width * scale_physical).max(0.0),
                    rotation: 0.0,
                    points: triangle.points,
                    inradius: triangle.inradius,
                }
            }
        };
        // A triangle turns as a rect does, so its gradient turns with it.
        unturned.turned(p.spin, lowering)
    }

    /// Reduce `p`, whose `geometry` is its lanes scaled to physical px, to physical space: its
    /// source snapped as a fill's, a drop shadow's moved and grown by its halo, not snapped again,
    /// so the source inside it lies on the same pixels as the fill.
    pub(super) fn shadow(
        p: &DrawShadowPayload,
        geometry: ShadowGeometry,
        lowering: &Lowering<'_>,
    ) -> Self {
        let source = Self::snapped(p.source.rect, p.spin, lowering);
        let corners = p
            .source
            .corners
            .fit_to(source.physical.size, lowering.transform.physical_scale());
        let rect = if p.shadow.inset() {
            source
        } else {
            let physical = Rect {
                min: source.physical.min + geometry.offset,
                size: source.physical.size,
            }
            .inflated(geometry.halo());
            ScaledRect::from_physical(physical, lowering.display.physical)
        };
        // A shadow has no border; its edge is the blur.
        Self::of_box(rect, corners, 0.0).turned(p.spin, lowering)
    }

    /// The logical `rect` in physical px: on the display's pixels, or exact when turned, as a
    /// turned rect's edges are not on the pixel grid.
    fn snapped(rect: Rect, spin: Option<Spin>, lowering: &Lowering<'_>) -> ScaledRect {
        let edges = match spin {
            None => lowering.display.edge_rounding(),
            Some(_) => EdgeRounding::Exact,
        };
        lowering.scaled_rect_with(rect, edges)
    }

    /// An unturned box over `rect`, rounded by `corners`, under a border `border_width` physical
    /// px wide.
    const fn of_box(rect: ScaledRect, corners: Corners, border_width: f32) -> Self {
        let size = rect.physical.size;
        Self {
            rect,
            corners,
            border_width,
            rotation: 0.0,
            points: Quad::box_points(size),
            inradius: Quad::box_inradius(size),
        }
    }

    /// This quad turned by `spin`: its rect moved and turned about its center as
    /// [`Lowering::turned_rect`] does, which keeps its size and so its shape.
    fn turned(self, spin: Option<Spin>, lowering: &Lowering<'_>) -> Self {
        match spin {
            None => self,
            Some(spin) => Self {
                rect: lowering.turned_rect(self.rect.physical, spin),
                rotation: spin.angle,
                ..self
            },
        }
    }
}
