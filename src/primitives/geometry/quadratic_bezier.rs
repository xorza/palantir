//! The quadratic Bézier [`Shape::quadratic_bezier`](crate::widget::Shape::quadratic_bezier) strokes; lowering promotes it to a [`CubicBezier`](crate::widget::CubicBezier).

use crate::primitives::math::domain::vec2;
use glam::Vec2;

/// A quadratic Bézier by its three control points, owner-local: `p0` and `p2` the ends, `p1` the pull.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadraticBezier {
    /// The start point.
    pub p0: Vec2,
    /// The pull both ends lean toward.
    pub p1: Vec2,
    /// The end point.
    pub p2: Vec2,
}

impl QuadraticBezier {
    /// Panics unless every control point is an [offset](crate::widget::domain::offset).
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(self) {
        vec2::offset(self.p0);
        vec2::offset(self.p1);
        vec2::offset(self.p2);
    }
}
