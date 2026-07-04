//! Bezier-curve utilities. Curves are rendered natively on the GPU
//! (see `renderer::backend::curve_pipeline`); CPU flattening is no
//! longer part of the pipeline. What remains: the quadratic→cubic
//! promotion the curve-lowering path uses to feed a single shader code
//! path, plus the curve-bbox helpers (`cubic_bezier_bbox` / `CurveBounds`
//! / `solve_quadratic`) that size the arena payload.

use glam::Vec2;

/// The two inner control points of a cubic Bezier.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CubicControls {
    pub(crate) c1: Vec2,
    pub(crate) c2: Vec2,
}

/// Axis-aligned bounds of a curve trace.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CurveBounds {
    pub(crate) lo: Vec2,
    pub(crate) hi: Vec2,
}

/// Promote a quadratic Bezier `(p0, c, p2)` to a cubic with the same
/// curve trace. Standard reparameterization: lift the inner two control
/// points to `p0 + 2/3·(c - p0)` and `p2 + 2/3·(c - p2)`. Exact, not an
/// approximation — every t in [0,1] evaluates to the same point on both
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
pub(crate) fn cubic_bezier_bbox(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> CurveBounds {
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
    CurveBounds { lo, hi }
}

/// Real roots of `a·t² + b·t + c = 0`. Returns `[NaN, NaN]` when there
/// are no real roots; the caller filters by `t ∈ (0, 1)` so NaNs drop
/// out naturally (NaN comparisons are false).
fn solve_quadratic(a: f32, b: f32, c: f32) -> [f32; 2] {
    if a.abs() < 1.0e-12 {
        if b.abs() < 1.0e-12 {
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
mod tests {
    use crate::primitives::bezier::*;

    #[test]
    fn quadratic_to_cubic_promotes_inner_cps() {
        let p0 = Vec2::new(0.0, 0.0);
        let c = Vec2::new(50.0, 100.0);
        let p2 = Vec2::new(100.0, 0.0);
        let CubicControls { c1: q1, c2: q2 } = quadratic_to_cubic(p0, c, p2);
        // q1 = p0 + 2/3·(c - p0) = (100/3, 200/3) ≈ (33.33, 66.67).
        // q2 = p2 + 2/3·(c - p2) = (200/3, 200/3) ≈ (66.67, 66.67).
        assert!((q1 - Vec2::new(100.0 / 3.0, 200.0 / 3.0)).length() < 1.0e-4);
        assert!((q2 - Vec2::new(200.0 / 3.0, 200.0 / 3.0)).length() < 1.0e-4);
    }

    #[test]
    fn cubic_bbox_is_endpoints_for_monotone_curve() {
        // Straight monotone curve along x: bbox = endpoint hull, no
        // contribution from inner CPs (which lie on the line).
        let p0 = Vec2::new(0.0, 0.0);
        let p1 = Vec2::new(33.0, 0.0);
        let p2 = Vec2::new(66.0, 0.0);
        let p3 = Vec2::new(100.0, 0.0);
        let CurveBounds { lo, hi } = cubic_bezier_bbox(p0, p1, p2, p3);
        assert!((lo - Vec2::new(0.0, 0.0)).length() < 1.0e-4);
        assert!((hi - Vec2::new(100.0, 0.0)).length() < 1.0e-4);
    }

    #[test]
    fn cubic_bbox_tighter_than_control_hull_for_opposing_tangents() {
        // S-curve: horizontal endpoints, inner CPs pulled vertically in
        // opposite directions. The actual curve excursion in y is far
        // smaller than the control-polygon hull (±100 → ±~38.5).
        let p0 = Vec2::new(0.0, 0.0);
        let p1 = Vec2::new(33.0, 100.0);
        let p2 = Vec2::new(66.0, -100.0);
        let p3 = Vec2::new(100.0, 0.0);
        let CurveBounds { lo, hi } = cubic_bezier_bbox(p0, p1, p2, p3);
        // Hull would give y ∈ [-100, 100]; tight bbox is ±25·√(1/3)·3 ≈ ±25/√3·... .
        // Don't pin the exact analytic value — just assert "well inside the hull".
        assert!(lo.y > -50.0, "lo.y = {}", lo.y);
        assert!(hi.y < 50.0, "hi.y = {}", hi.y);
        // Symmetric S: lo.y == -hi.y up to fp slop.
        assert!((lo.y + hi.y).abs() < 1.0e-3);
        // Endpoints always included.
        assert!((lo.x - 0.0).abs() < 1.0e-4);
        assert!((hi.x - 100.0).abs() < 1.0e-4);
    }

    #[test]
    fn cubic_bbox_contains_sampled_curve() {
        // Stress: random-ish CPs; verify all sampled curve points lie
        // inside the reported bbox.
        let p0 = Vec2::new(10.0, 20.0);
        let p1 = Vec2::new(-30.0, 80.0);
        let p2 = Vec2::new(120.0, -40.0);
        let p3 = Vec2::new(90.0, 50.0);
        let CurveBounds { lo, hi } = cubic_bezier_bbox(p0, p1, p2, p3);
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            let u = 1.0 - t;
            let p = u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3;
            assert!(
                p.x >= lo.x - 1.0e-3 && p.x <= hi.x + 1.0e-3,
                "x at t={t}: {}",
                p.x
            );
            assert!(
                p.y >= lo.y - 1.0e-3 && p.y <= hi.y + 1.0e-3,
                "y at t={t}: {}",
                p.y
            );
        }
    }

    #[test]
    fn quadratic_to_cubic_matches_midpoint() {
        // Quadratic Q(t) at t=0.5: 0.25·p0 + 0.5·c + 0.25·p2.
        // Cubic C(t) at t=0.5: 0.125·p0 + 0.375·q1 + 0.375·q2 + 0.125·p2.
        // For the promoted (q1, q2), C(0.5) == Q(0.5).
        let p0 = Vec2::new(1.0, 2.0);
        let c = Vec2::new(10.0, 30.0);
        let p2 = Vec2::new(-5.0, 7.0);
        let CubicControls { c1: q1, c2: q2 } = quadratic_to_cubic(p0, c, p2);
        let q_mid = 0.25 * p0 + 0.5 * c + 0.25 * p2;
        let c_mid = 0.125 * p0 + 0.375 * q1 + 0.375 * q2 + 0.125 * p2;
        assert!((q_mid - c_mid).length() < 1.0e-5);
    }
}
