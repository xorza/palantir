//! [`TemplateShape`]: a closed draw reduced to physical px, as the shape template takes it: the
//! one packer of a fill, a shaped image, an effect and a shadow's box, past which the composer is
//! shape-blind.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::num::F32Px;
use crate::primitives::packed::half_simd::F16x4;
use crate::renderer::frontend::composer::lowering::{Lowering, ScaledRect};
use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::Vec2;

/// A closed shape in physical px as the shape template takes it: its covering rect and the whole
/// pixels it reaches, its sharp points relative to that rect, a radius at each, its inradius, and
/// a paint animation's turn about the rect's center (`ShapeHeader`).
#[derive(Debug)]
pub(super) struct TemplateShape {
    /// The shape's own rect, unturned, moved by a turn about its pivot, and the whole pixels it
    /// reaches turned.
    pub(super) rect: ScaledRect,
    /// The shape's sharp points from the rect's top-left, clockwise on screen.
    pub(super) points: [Vec2; 4],
    /// A radius at each point, `(tl, tr, br, bl)` for a box.
    pub(super) corners: Corners,
    /// The shape's inradius (`ShapeHeader::inradius`).
    pub(super) inradius: f32,
    /// A paint animation's turn about the rect's center, in radians; 0 for none.
    pub(super) rotation: f32,
}

impl TemplateShape {
    /// `placed` packed in physical px, unturned: a box's rect on the pixels [`Self::edges`] picks,
    /// its corners fit to it; or its triangle. [`Self::turned`] then turns it by `spin`, whose
    /// presence alone picks the edges here.
    // Always inlined: as a call, `repeated_chrome` ran 4 % slower.
    #[inline(always)]
    pub(super) fn of(placed: PlacedClosed, spin: Option<Spin>, lowering: &Lowering<'_>) -> Self {
        match placed {
            PlacedClosed::Box {
                placed: PlacedBox { rect, corners },
                ..
            } => {
                let rect = lowering.scaled_rect_with(rect, Self::edges(placed, spin, lowering));
                let corners =
                    corners.fit_to(rect.physical.size, lowering.transform.physical_scale());
                Self::of_box(rect, corners)
            }
            PlacedClosed::Triangle(placed) => {
                Self::triangle(placed.centers, placed.radius, lowering)
            }
        }
    }

    /// The box `p` casts its shadow from, packed as a fill's box is: a drop shadow's moved by its
    /// offset and grown by its halo, of `geometry`, its lanes in physical px, and not snapped
    /// again, so the source inside it lies on the same pixels as the fill. A shadow draws through
    /// the blurred-box family, not the template, which reads the rect and the corners alone.
    pub(super) fn shadow(
        p: &DrawShadowPayload,
        geometry: ShadowGeometry,
        lowering: &Lowering<'_>,
    ) -> Self {
        let source = Self::of(PlacedClosed::inside(p.source), p.spin, lowering);
        if p.shadow.inset() {
            return source.turned(p.spin, lowering);
        }
        let grown = Rect {
            min: source.rect.physical.min + geometry.offset,
            size: source.rect.physical.size,
        }
        .inflated(geometry.halo());
        Self::of_box(
            ScaledRect::from_physical(grown, lowering.display.physical),
            source.corners,
        )
        .turned(p.spin, lowering)
    }

    /// How `placed`'s edges round: a still box's to the display's pixels; a turned box's
    /// exactly, as its edges are not on the pixel grid, and a triangle's, which places itself to
    /// the sub-pixel.
    pub(super) const fn edges(
        placed: PlacedClosed,
        spin: Option<Spin>,
        lowering: &Lowering<'_>,
    ) -> EdgeRounding {
        match (placed, spin) {
            (PlacedClosed::Box { .. }, None) => lowering.display.edge_rounding(),
            _ => EdgeRounding::Exact,
        }
    }

    /// This shape turned by `spin`: its rect moved and turned about its center as
    /// [`Lowering::turned_rect`] does, which keeps its size and so its points.
    pub(super) fn turned(self, spin: Option<Spin>, lowering: &Lowering<'_>) -> Self {
        match spin {
            None => self,
            Some(spin) => Self {
                rect: lowering.turned_rect(self.rect.physical, spin),
                rotation: spin.angle,
                ..self
            },
        }
    }

    /// The shape as an instance opens with it, cut by the rounded clips of `clip`.
    pub(super) const fn header(&self, clip: ClipChain) -> ShapeHeader {
        ShapeHeader {
            rect: self.rect.physical,
            points: self.points,
            corners: self.corners,
            inradius: self.inradius,
            rotation: self.rotation,
            clip,
        }
    }

    /// Unturned under a border `border_width` physical px wide, with no rounded corner: the edges
    /// are the rect's.
    pub(super) const fn is_sharp(&self, border_width: f32) -> bool {
        self.rotation == 0.0 && is_invisible(border_width) && self.corners.is_approx_zero()
    }

    /// Unturned, with no rounded corner, and physical edges on whole pixels: the template's outer
    /// coverage is then whole at every pixel it shades, with no antialiased edge, whatever border
    /// lies inside it.
    pub(super) fn is_pixel_aligned(&self) -> bool {
        let physical = self.rect.physical;
        let max = physical.max();
        self.rotation == 0.0
            && self.corners.is_approx_zero()
            && physical.min.x.is_integral()
            && physical.min.y.is_integral()
            && max.x.is_integral()
            && max.y.is_integral()
    }

    /// An unturned box over `rect`, rounded by `corners`, already fit to it.
    const fn of_box(rect: ScaledRect, corners: Corners) -> Self {
        let size = rect.physical.size;
        Self {
            rect,
            points: ShapeHeader::box_points(size),
            corners,
            inradius: ShapeHeader::box_inradius(size),
            rotation: 0.0,
        }
    }

    /// The triangle of logical corner centers `points` dilated by `radius`, its rect unsnapped,
    /// as the shape places itself to the sub-pixel, its fourth point repeating the third.
    fn triangle(points: [Vec2; 3], radius: f32, lowering: &Lowering<'_>) -> Self {
        let to_physical = lowering.transform.physical_map(Vec2::ZERO);
        let [a, b, c] = points.map(|q| to_physical.point(q));
        // Saturated as the f16 lane holds it, so the quad and the shader take one radius.
        let radius_physical =
            F16x4::saturate_lane((radius * lowering.transform.physical_scale()).max(0.0));
        // The rounded shape's bounds, which its gradient and its image span; a border sits on the
        // inner edge, so it adds no outward reach.
        let rect = Rect::from_min_max(a.min(b).min(c), a.max(b).max(c)).inflated(radius_physical);
        let grown = GrownTriangle::of([a, b, c], radius_physical);
        let [p0, p1, p2] = grown.points.map(|q| q - rect.min);
        Self {
            rect: ScaledRect::from_physical(rect, lowering.display.physical),
            points: [p0, p1, p2, p2],
            corners: Corners::saturating([radius_physical, radius_physical, radius_physical, 0.0]),
            inradius: grown.inradius,
            rotation: 0.0,
        }
    }
}

/// A triangle grown by a radius, as `sdf_triangle - radius` draws it: its corners are arcs of the
/// radius about its points, so its sharp polygon's corners lie that radius past them along each
/// corner's bisector, and the template rounds them back about the points.
#[derive(Debug)]
struct GrownTriangle {
    /// The grown polygon's corners, clockwise on screen.
    points: [Vec2; 3],
    /// The grown polygon's inradius: the triangle's own plus the growth.
    inradius: f32,
}

impl GrownTriangle {
    fn of([a, b, c]: [Vec2; 3], radius: f32) -> Self {
        // Clockwise on screen, y down, is a positive cross product, as a box's `(tl, tr, br)`.
        let cross = (b - a).perp_dot(c - a);
        let [a, b, c] = if cross > 0.0 { [a, b, c] } else { [a, c, b] };
        let sharp = [a, b, c];
        // The outward normal of the edge from `p` to `q`, as the template takes it.
        let normal = |p: Vec2, q: Vec2| {
            let d = q - p;
            Vec2::new(d.y, -d.x) / d.length()
        };
        let points = std::array::from_fn(|i| {
            let p = sharp[i];
            let na = normal(sharp[(i + 2) % 3], p);
            let nb = normal(p, sharp[(i + 1) % 3]);
            p + radius * (na + nb) / (1.0 + na.dot(nb))
        });
        let perimeter = (b - a).length() + (c - b).length() + (a - c).length();
        Self {
            points,
            inradius: cross.abs() / perimeter + radius,
        }
    }
}
