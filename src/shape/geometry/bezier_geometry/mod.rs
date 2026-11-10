//! [`BezierGeometry`]: a curve record's geometry, which fits its owner where it is drawn.

use crate::common::hash::Hasher;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain::vec2;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::line_style::LineCap;
use crate::shape::geometry::curve_basis::CurveBasis;
use crate::shape::geometry::placed_stroke::PlacedStroke;
use crate::shape::geometry::point_placement::PointPlacement;

/// A cubic Bézier, its points logical px in its placement's rect, which a line and a quadratic lower
/// to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BezierGeometry {
    pub(crate) bezier: CubicBezier,
    pub(crate) placement: PointPlacement,
}

impl BezierGeometry {
    /// What the curve draws in an owner of `owner_size`, ended by `cap`, `width` across. The fit
    /// maps the control points, which an axis scale keeps on the curve; a curve all of whose
    /// points meet paints its cap.
    pub(crate) fn place(self, owner_size: Size, cap: LineCap, width: f32) -> PlacedStroke {
        let map = self.placement.map(self.bezier.bbox(), owner_size);
        let CubicBezier { p0, p1, p2, p3 } = self.bezier;
        let [p0, p1, p2, p3] = [p0, p1, p2, p3].map(|p| map.apply(p));
        if vec2::approx_eq(p0, p1) && vec2::approx_eq(p0, p2) && vec2::approx_eq(p0, p3) {
            return PlacedStroke::dot(p0, cap, width);
        }
        PlacedStroke::Curve(CurveBasis::Cubic(CubicBezier { p0, p1, p2, p3 }))
    }

    /// Feed the geometry into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        let CubicBezier { p0, p1, p2, p3 } = self.bezier;
        for point in [p0, p1, p2, p3] {
            point.hash_visual(h);
        }
        self.placement.hash_into(h);
    }
}

impl NanCheck for BezierGeometry {
    fn has_nan(&self) -> bool {
        let CubicBezier { p0, p1, p2, p3 } = self.bezier;
        [p0, p1, p2, p3].iter().any(|p| p.is_nan()) || self.placement.has_nan()
    }
}

#[cfg(test)]
mod tests;
