//! [`TriangleGeometry`]: a triangle record's corners, which fit its owner where it is drawn.

use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::shape::geometry::point_placement::PointPlacement;
use glam::Vec2;

/// A triangle's corners, logical px in its placement's rect, and the radius that rounds them; the
/// radius does not scale with the fit, as a stroke's width does not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TriangleGeometry {
    pub(crate) corners: [Vec2; 3],
    pub(crate) radius: f32,
    pub(crate) placement: PointPlacement,
}

/// A triangle placed in its owner, owner-local: the corner centers whose dilation by `radius`
/// is the shape, and that shape's box. The AA fringe is added later in `cascade::paint_rect`,
/// since it is half a physical pixel and this box is logical.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacedTriangle {
    pub(crate) centers: [Vec2; 3],
    pub(crate) radius: f32,
    pub(crate) bbox: Rect,
}

impl TriangleGeometry {
    /// The triangle in an owner of `owner_size`; `None` where its fitted area paints nothing, as
    /// in a rect of no size.
    pub(crate) fn place(self, owner_size: Size) -> Option<PlacedTriangle> {
        let map = self.placement.map(Aabb::of(&self.corners), owner_size);
        let corners = self.corners.map(|p| map.apply(p));
        if area_is_paint_empty(corners) {
            return None;
        }
        let CornerCenters { points, radius } = CornerCenters::new(corners, self.radius);
        Some(PlacedTriangle {
            centers: points,
            radius,
            bbox: Aabb::of(&points).inflated(radius),
        })
    }

    /// Feed the geometry into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        for corner in self.corners {
            corner.hash_visual(h);
        }
        self.radius.hash_visual(h);
        self.placement.hash_into(h);
    }
}

/// Whether the triangle `corners` paints nothing: twice its area over its longest edge squared, a
/// measure no uniform scale moves, rounds to none. Rounding stays inside the triangle.
pub(crate) fn area_is_paint_empty([a, b, c]: [Vec2; 3]) -> bool {
    let ab = b - a;
    let ac = c - a;
    let bc = c - b;
    let max_edge_len_sq = ab
        .length_squared()
        .max(ac.length_squared())
        .max(bc.length_squared());
    is_invisible(ab.perp_dot(ac).abs() / max_edge_len_sq)
}

/// `radius` is checked apart, as the clamp to the inradius, an `f32::min`, launders a NaN.
impl NanCheck for TriangleGeometry {
    fn has_nan(&self) -> bool {
        self.corners.iter().any(|corner| corner.is_nan())
            || self.radius.is_nan()
            || self.placement.has_nan()
    }
}

/// The triangle whose dilation by `radius` rounds a sharp one's corners and keeps its edges.
///
/// Each edge moves in by the radius. Every edge line lies one inradius from the incenter, so the
/// move is the homothety about the incenter of ratio `1 - radius / inradius`, the inset Inigo
/// Quilez's rounded box takes before it subtracts the radius
/// (<https://iquilezles.org/articles/distfunctions2d/>). The radius clamps to the inradius,
/// where the centers meet at the incenter and the dilation is the incircle.
#[derive(Debug)]
struct CornerCenters {
    points: [Vec2; 3],
    radius: f32,
}

impl CornerCenters {
    fn new(corners: [Vec2; 3], radius: f32) -> Self {
        let [a, b, c] = corners;
        let side_a = b.distance(c);
        let side_b = c.distance(a);
        let side_c = a.distance(b);
        let perimeter = side_a + side_b + side_c;
        let incenter = (a * side_a + b * side_b + c * side_c) / perimeter;
        let inradius = (b - a).perp_dot(c - a).abs() / perimeter;
        debug_assert!(inradius > 0.0, "`place` refuses a triangle with no area");
        let radius = radius.min(inradius);
        // `v + (incenter - v) * t` rather than a lerp from the incenter: at no radius it returns
        // each corner bit for bit.
        let t = radius / inradius;
        Self {
            points: corners.map(|v| v + (incenter - v) * t),
            radius,
        }
    }
}

#[cfg(test)]
mod tests;
