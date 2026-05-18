//! Bezier-curve utilities. Curves are rendered natively on the GPU
//! (see `renderer::backend::curve_pipeline`); CPU flattening is no
//! longer part of the pipeline. The only thing left here is the
//! quadratic→cubic promotion the curve-lowering path uses to feed a
//! single shader code path.

use glam::Vec2;

/// Promote a quadratic Bezier `(p0, c, p2)` to a cubic with the same
/// curve trace. Standard reparameterization: lift the inner two control
/// points to `p0 + 2/3·(c - p0)` and `p2 + 2/3·(c - p2)`. Exact, not an
/// approximation — every t in [0,1] evaluates to the same point on both
/// forms.
#[inline]
pub(crate) fn quadratic_to_cubic(p0: Vec2, c: Vec2, p2: Vec2) -> (Vec2, Vec2) {
    let q1 = p0 + (c - p0) * (2.0 / 3.0);
    let q2 = p2 + (c - p2) * (2.0 / 3.0);
    (q1, q2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadratic_to_cubic_promotes_inner_cps() {
        let p0 = Vec2::new(0.0, 0.0);
        let c = Vec2::new(50.0, 100.0);
        let p2 = Vec2::new(100.0, 0.0);
        let (q1, q2) = quadratic_to_cubic(p0, c, p2);
        // q1 = p0 + 2/3·(c - p0) = (100/3, 200/3) ≈ (33.33, 66.67).
        // q2 = p2 + 2/3·(c - p2) = (200/3, 200/3) ≈ (66.67, 66.67).
        assert!((q1 - Vec2::new(100.0 / 3.0, 200.0 / 3.0)).length() < 1.0e-4);
        assert!((q2 - Vec2::new(200.0 / 3.0, 200.0 / 3.0)).length() < 1.0e-4);
    }

    #[test]
    fn quadratic_to_cubic_matches_midpoint() {
        // Quadratic Q(t) at t=0.5: 0.25·p0 + 0.5·c + 0.25·p2.
        // Cubic C(t) at t=0.5: 0.125·p0 + 0.375·q1 + 0.375·q2 + 0.125·p2.
        // For the promoted (q1, q2), C(0.5) == Q(0.5).
        let p0 = Vec2::new(1.0, 2.0);
        let c = Vec2::new(10.0, 30.0);
        let p2 = Vec2::new(-5.0, 7.0);
        let (q1, q2) = quadratic_to_cubic(p0, c, p2);
        let q_mid = 0.25 * p0 + 0.5 * c + 0.25 * p2;
        let c_mid = 0.125 * p0 + 0.375 * q1 + 0.375 * q2 + 0.125 * p2;
        assert!((q_mid - c_mid).length() < 1.0e-5);
    }
}
