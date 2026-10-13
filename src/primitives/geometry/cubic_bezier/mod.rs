//! The cubic Bézier [`Shape::cubic_bezier`](crate::widget::Shape::cubic_bezier) strokes and lowering reduces lines and quadratics to, and the tight bbox that sizes its arena payload.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::vec2;
use glam::Vec2;

/// A cubic Bézier by its four control points, owner-local: `p0` and `p3` the ends, `p1` and `p2` the inner pulls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    /// The start point.
    pub p0: Vec2,
    /// The pull leaving `p0`.
    pub p1: Vec2,
    /// The pull arriving at `p3`.
    pub p2: Vec2,
    /// The end point.
    pub p3: Vec2,
}

impl CubicBezier {
    /// Panics unless every control point is an [offset](crate::widget::domain::offset).
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(self) {
        vec2::offset(self.p0);
        vec2::offset(self.p1);
        vec2::offset(self.p2);
        vec2::offset(self.p3);
    }

    /// The cubic tracing the quadratic `(p0, c, p2)` exactly: inner points `p0 + 2/3·(c - p0)` and `p2 + 2/3·(c - p2)`.
    #[inline]
    pub(crate) fn from_quadratic(p0: Vec2, c: Vec2, p2: Vec2) -> Self {
        Self {
            p0,
            p1: p0 + (c - p0) * (2.0 / 3.0),
            p2: p2 + (c - p2) * (2.0 / 3.0),
            p3: p2,
        }
    }

    /// Tight bbox of the curve trace, not its control polygon (which overstates the extent). Solve `B'(t) = 0` per axis, keep roots in `(0, 1)`, combine with the endpoints.
    ///
    /// `B'(t)/3 = (p1 - p0) + 2t(p0 - 2p1 + p2) + t²(-p0 + 3p1 - 3p2 + p3)`,
    /// so per axis: `a = -p0 + 3p1 - 3p2 + p3`, `b = 2(p0 - 2p1 + p2)`,
    /// `c = p1 - p0`.
    pub(crate) fn bbox(self) -> Rect {
        let Self { p0, p1, p2, p3 } = self;
        // One up-front NaN screen: `min`/`max` would drop a NaN endpoint and the `t` filter rejects NaN roots, so an interior control point would go unseen; folding it into the bounds would loosen the box to the control hull.
        if p0.is_nan() || p1.is_nan() || p2.is_nan() || p3.is_nan() {
            return Rect::NAN;
        }
        let mut lo = p0.min(p3);
        let mut hi = p0.max(p3);
        for axis in 0..2 {
            let v0 = p0[axis];
            let v1 = p1[axis];
            let v2 = p2[axis];
            let v3 = p3[axis];
            let a = -v0 + 3.0 * v1 - 3.0 * v2 + v3;
            let b = 2.0 * (v0 - 2.0 * v1 + v2);
            let c = v1 - v0;
            for &t in &solve_quadratic(a, b, c) {
                if t > 0.0 && t < 1.0 {
                    let u = 1.0 - t;
                    let val = u * u * u * v0
                        + 3.0 * u * u * t * v1
                        + 3.0 * u * t * t * v2
                        + t * t * t * v3;
                    if val < lo[axis] {
                        lo[axis] = val;
                    }
                    if val > hi[axis] {
                        hi[axis] = val;
                    }
                }
            }
        }
        Rect::from_min_max(lo, hi)
    }
}

/// Real roots of `a·t² + b·t + c = 0`; `[NaN, NaN]` when none, which the caller's `t ∈ (0, 1)` filter drops.
///
/// Cancellation-free form (Numerical Recipes §5.6): `q = -½·(b + sign(b)·√disc)`, roots `q/a` and `c/q`. The textbook form loses the small root when `|a|` is small against `|b|`, as it is for every promoted quadratic, leaving the extremum outside the bbox.
fn solve_quadratic(a: f32, b: f32, c: f32) -> [f32; 2] {
    /// Below this a coefficient carries no root worth recovering.
    ///
    /// Not the visual `EPS`: these are differences of control-point coordinates, so dividing by a term this small manufactures a root from rounding noise.
    const NEGLIGIBLE_COEFF: f32 = 1.0e-12;
    if a.abs() < NEGLIGIBLE_COEFF {
        if b.abs() < NEGLIGIBLE_COEFF {
            return [f32::NAN, f32::NAN];
        }
        return [-c / b, f32::NAN];
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return [f32::NAN, f32::NAN];
    }
    let q = -0.5 * (b + disc.sqrt().copysign(b));
    // `q == 0` means the double root at 0: `q/a` gives it, `c/q` reports NaN.
    [q / a, c / q]
}

#[cfg(test)]
mod tests;
