//! [`TemplateShape`]: a closed shape in physical px as the shape template takes it, which a
//! quad's triangle, a shaped image and an effect pack through.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::packed::half_simd::F16x4;
use crate::renderer::frontend::composer::lowering::Lowering;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::quad::Quad;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use glam::Vec2;

/// A closed shape in physical px as the shape template takes it: its covering rect, its sharp
/// points relative to that rect, a radius at each, and its inradius. A quad's triangle and an image's
/// shape both pack through it.
#[derive(Debug)]
pub(super) struct TemplateShape {
    pub(super) rect: Rect,
    pub(super) points: [Vec2; 4],
    pub(super) corners: Corners,
    pub(super) inradius: f32,
}

impl TemplateShape {
    /// `placed` packed in physical px, turned by `spin` or not: a box's rect on the pixels
    /// [`Self::edges`] picks, its corners fit to it; or its triangle.
    pub(super) fn of(placed: PlacedClosed, spin: Option<Spin>, lowering: &Lowering<'_>) -> Self {
        match placed {
            PlacedClosed::Box(PlacedBox { rect, corners }) => {
                let rect = lowering
                    .scaled_rect_with(rect, Self::edges(placed, spin, lowering))
                    .physical;
                Self::rounded_rect(
                    rect,
                    corners.fit_to(rect.size, lowering.transform.physical_scale()),
                )
            }
            PlacedClosed::Triangle(placed) => {
                Self::triangle(placed.centers, placed.radius, lowering)
            }
        }
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
            (PlacedClosed::Box(_), None) => lowering.display.edge_rounding(),
            _ => EdgeRounding::Exact,
        }
    }

    /// The physical `rect` rounded by `corners`, already fit to it.
    pub(super) const fn rounded_rect(rect: Rect, corners: Corners) -> Self {
        Self {
            rect,
            points: Quad::box_points(rect.size),
            corners,
            inradius: Quad::box_inradius(rect.size),
        }
    }

    /// The triangle of logical corner centers `points` dilated by `radius`, its rect unsnapped,
    /// as the shape places itself to the sub-pixel, its fourth point repeating the third.
    pub(super) fn triangle(points: [Vec2; 3], radius: f32, lowering: &Lowering<'_>) -> Self {
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
            rect,
            points: [p0, p1, p2, p2],
            corners: Corners::saturating([radius_physical, radius_physical, radius_physical, 0.0]),
            inradius: grown.inradius,
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
