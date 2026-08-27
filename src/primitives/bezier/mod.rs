//! Bezier-curve utilities. Curves are rendered natively on the GPU
//! (see `renderer::backend::curve_pipeline`); CPU flattening is no
//! longer part of the pipeline. What remains: the quadratic→cubic
//! promotion the curve-lowering path uses to feed a single shader code
//! path, plus the curve-bbox helpers (`cubic_bezier_bbox` /
//! `solve_quadratic`) that size the arena payload.

use crate::primitives::rect::Rect;
use glam::Vec2;

/// The two inner control points of a cubic Bezier.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CubicControls {
    pub(crate) c1: Vec2,
    pub(crate) c2: Vec2,
}

/// Promote a quadratic Bezier `(p0, c, p2)` to a cubic with the same
/// curve trace. Standard reparameterization: lift the inner two control
/// points to `p0 + 2/3·(c - p0)` and `p2 + 2/3·(c - p2)`. Exact, not an
/// approximation — every t in `[0, 1]` evaluates to the same point on both
/// forms.
#[inline]
pub(crate) fn quadratic_to_cubic(p0: Vec2, c: Vec2, p2: Vec2) -> CubicControls {
    CubicControls {
        c1: p0 + (c - p0) * (2.0 / 3.0),
        c2: p2 + (c - p2) * (2.0 / 3.0),
    }
}

/// Tight axis-aligned bbox of the cubic Bezier curve trace (not the
/// control polygon). The control-polygon hull is conservative but loose:
/// when inner CPs point in opposite directions, it overstates the painted
/// extent significantly. Solve `B'(t) = 0` per axis (a quadratic in t),
/// keep roots in `(0, 1)`, and combine with the endpoints.
///
/// `B'(t)/3 = (p1 - p0) + 2t(p0 - 2p1 + p2) + t²(-p0 + 3p1 - 3p2 + p3)`,
/// so per axis: `a = -p0 + 3p1 - 3p2 + p3`, `b = 2(p0 - 2p1 + p2)`,
/// `c = p1 - p0`.
pub(crate) fn cubic_bezier_bbox(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> Rect {
    // Four fixed inputs, so the AABB NaN contract is cheapest as one
    // up-front screen — no per-point flag to carry, and `min`/`max`
    // below stay the plain laundering form. It has to be up front:
    // `min`/`max` would drop a NaN endpoint, and an interior control
    // point reaches neither that seed nor the extremum scan (the `t`
    // filter rejects NaN roots, since NaN compares false). Folding the
    // interior points into the bounds instead would catch them, but
    // would also loosen the box to the control hull — the exact
    // tightness this function exists for.
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
                let val =
                    u * u * u * v0 + 3.0 * u * u * t * v1 + 3.0 * u * t * t * v2 + t * t * t * v3;
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

/// Real roots of `a·t² + b·t + c = 0`. Returns `[NaN, NaN]` when there
/// are no real roots; the caller filters by `t ∈ (0, 1)` so NaNs drop
/// out naturally (NaN comparisons are false).
fn solve_quadratic(a: f32, b: f32, c: f32) -> [f32; 2] {
    /// Below this a coefficient carries no root worth recovering.
    ///
    /// Not the crate's visual [`EPS`](crate::primitives::approx::EPS),
    /// which answers a question about painted distance: these
    /// coefficients are differences of control-point coordinates in a
    /// derivative, so their scale is the curve's, not the screen's, and a
    /// term this far below it is numerically absent. Dividing by it
    /// manufactures a root out of rounding noise instead — which is what
    /// the threshold exists to stop, and why it sits eight orders below
    /// the visual one.
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
    let s = disc.sqrt();
    [(-b + s) / (2.0 * a), (-b - s) / (2.0 * a)]
}

#[cfg(test)]
mod tests;
