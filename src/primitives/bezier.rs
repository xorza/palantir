//! Bezier flattening. Pure math, no framework deps. Adaptive
//! recursive subdivision with a control-polygon-to-chord deviation
//! test — the standard approach (e.g. Sederberg, Computer Aided
//! Geometric Design).
//!
//! Output points carry their parametric `t` so a future
//! color-bezier evaluator can match the curve's t-parametrization
//! (denser around curvature peaks, sparser in flat regions). Solid
//! color only for now; t is unused by the current consumer.

use crate::primitives::approx::EPS;
use glam::Vec2;

/// Hard cap on recursive subdivision depth. `2^20 ≈ 1M` segments
/// is well past any realistic tolerance × curve-length the renderer
/// would emit; the cap is a safety net, not a quality knob.
const MAX_DEPTH: u8 = 20;

/// One flattened sample: the point on the curve plus its parametric
/// `t` in `[0, 1]`. Threaded through recursion so color modes can
/// evaluate a color-bezier at the same t.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlatPoint {
    pub(crate) p: Vec2,
    pub(crate) t: f32,
}

/// Flatten a cubic Bezier into a polyline. Appends to `out` —
/// caller controls allocation. `tolerance` is the maximum allowed
/// perpendicular distance, in the curve's own units (logical px at
/// authoring time), from any control point to the chord; smaller =
/// more segments. Values `<= EPS` are clamped to `EPS`.
///
/// Always emits at least 2 points (start + end). Endpoints land
/// exactly on `p0` / `p3` with `t = 0.0` / `t = 1.0` (no FP drift
/// at the boundaries — important for color modes that key off t).
pub(crate) fn flatten_cubic(
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    p3: Vec2,
    tolerance: f32,
    out: &mut Vec<FlatPoint>,
) {
    let tol = tolerance.max(EPS);
    let tol_sq = tol * tol;
    out.push(FlatPoint { p: p0, t: 0.0 });
    flatten_cubic_recurse(p0, p1, p2, p3, 0.0, 1.0, tol_sq, out, MAX_DEPTH);
    out.push(FlatPoint { p: p3, t: 1.0 });
}

/// Flatten a quadratic Bezier into a polyline. Same contract as
/// [`flatten_cubic`].
pub(crate) fn flatten_quadratic(
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    tolerance: f32,
    out: &mut Vec<FlatPoint>,
) {
    let tol = tolerance.max(EPS);
    let tol_sq = tol * tol;
    out.push(FlatPoint { p: p0, t: 0.0 });
    flatten_quadratic_recurse(p0, p1, p2, 0.0, 1.0, tol_sq, out, MAX_DEPTH);
    out.push(FlatPoint { p: p2, t: 1.0 });
}

#[allow(clippy::too_many_arguments)]
fn flatten_cubic_recurse(
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    p3: Vec2,
    t_lo: f32,
    t_hi: f32,
    tol_sq: f32,
    out: &mut Vec<FlatPoint>,
    depth: u8,
) {
    if depth == 0 || cubic_is_flat(p0, p1, p2, p3, tol_sq) {
        // Don't push p3 here — outer caller (or sibling) emits it.
        // Recursion instead emits the *interior* split midpoint
        // produced by de Casteljau; see the split arms below.
        return;
    }
    let q0 = (p0 + p1) * 0.5;
    let q1 = (p1 + p2) * 0.5;
    let q2 = (p2 + p3) * 0.5;
    let r0 = (q0 + q1) * 0.5;
    let r1 = (q1 + q2) * 0.5;
    let s = (r0 + r1) * 0.5;
    let t_mid = (t_lo + t_hi) * 0.5;
    flatten_cubic_recurse(p0, q0, r0, s, t_lo, t_mid, tol_sq, out, depth - 1);
    out.push(FlatPoint { p: s, t: t_mid });
    flatten_cubic_recurse(s, r1, q2, p3, t_mid, t_hi, tol_sq, out, depth - 1);
}

#[allow(clippy::too_many_arguments)]
fn flatten_quadratic_recurse(
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    t_lo: f32,
    t_hi: f32,
    tol_sq: f32,
    out: &mut Vec<FlatPoint>,
    depth: u8,
) {
    if depth == 0 || quadratic_is_flat(p0, p1, p2, tol_sq) {
        return;
    }
    let q0 = (p0 + p1) * 0.5;
    let q1 = (p1 + p2) * 0.5;
    let s = (q0 + q1) * 0.5;
    let t_mid = (t_lo + t_hi) * 0.5;
    flatten_quadratic_recurse(p0, q0, s, t_lo, t_mid, tol_sq, out, depth - 1);
    out.push(FlatPoint { p: s, t: t_mid });
    flatten_quadratic_recurse(s, q1, p2, t_mid, t_hi, tol_sq, out, depth - 1);
}

#[inline]
fn cubic_is_flat(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, tol_sq: f32) -> bool {
    dist_sq_to_line(p1, p0, p3) <= tol_sq && dist_sq_to_line(p2, p0, p3) <= tol_sq
}

#[inline]
fn quadratic_is_flat(p0: Vec2, p1: Vec2, p2: Vec2, tol_sq: f32) -> bool {
    dist_sq_to_line(p1, p0, p2) <= tol_sq
}

/// Squared perpendicular distance from `p` to the infinite line
/// through `a`/`b`. If `a == b`, falls back to squared distance to
/// the point itself — keeps flatness test well-defined for
/// degenerate (coincident-endpoints) curves.
#[inline]
fn dist_sq_to_line(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let len_sq = ab.length_squared();
    if len_sq < 1.0e-20 {
        return ap.length_squared();
    }
    let cross = ab.x * ap.y - ab.y * ap.x;
    cross * cross / len_sq
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubic_straight_line_flattens_to_two_points() {
        // Control points collinear and evenly spaced → curve is the chord itself.
        let mut out = Vec::new();
        flatten_cubic(
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(3.0, 0.0),
            0.25,
            &mut out,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].p, Vec2::new(0.0, 0.0));
        assert_eq!(out[0].t, 0.0);
        assert_eq!(out[1].p, Vec2::new(3.0, 0.0));
        assert_eq!(out[1].t, 1.0);
    }

    #[test]
    fn quadratic_straight_line_flattens_to_two_points() {
        let mut out = Vec::new();
        flatten_quadratic(
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 0.0),
            0.25,
            &mut out,
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn cubic_t_monotone_and_endpoints_exact() {
        let mut out = Vec::new();
        flatten_cubic(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 50.0),
            Vec2::new(90.0, 50.0),
            Vec2::new(100.0, 0.0),
            0.25,
            &mut out,
        );
        assert!(out.len() >= 4);
        assert_eq!(out.first().unwrap().t, 0.0);
        assert_eq!(out.last().unwrap().t, 1.0);
        for w in out.windows(2) {
            assert!(w[0].t < w[1].t, "t not strictly monotone: {:?}", out);
        }
    }

    #[test]
    fn cubic_symmetric_curve_flattens_symmetrically() {
        // Symmetric S-arch about x=50.
        let mut out = Vec::new();
        flatten_cubic(
            Vec2::new(0.0, 0.0),
            Vec2::new(20.0, 80.0),
            Vec2::new(80.0, 80.0),
            Vec2::new(100.0, 0.0),
            0.5,
            &mut out,
        );
        let n = out.len();
        assert!(n >= 3);
        // Point count is odd (midpoint emitted), and pairs mirror about t=0.5.
        for i in 0..n / 2 {
            let a = out[i];
            let b = out[n - 1 - i];
            assert!(
                (a.t + b.t - 1.0).abs() < 1.0e-5,
                "t pair not symmetric: {:?} {:?}",
                a,
                b,
            );
            assert!(
                (a.p.x + b.p.x - 100.0).abs() < 1.0e-3,
                "x not symmetric: {:?} {:?}",
                a,
                b,
            );
            assert!(
                (a.p.y - b.p.y).abs() < 1.0e-3,
                "y not symmetric: {:?} {:?}",
                a,
                b,
            );
        }
    }

    #[test]
    fn tighter_tolerance_produces_more_points() {
        let p0 = Vec2::new(0.0, 0.0);
        let p1 = Vec2::new(20.0, 80.0);
        let p2 = Vec2::new(80.0, 80.0);
        let p3 = Vec2::new(100.0, 0.0);
        let mut coarse = Vec::new();
        let mut fine = Vec::new();
        flatten_cubic(p0, p1, p2, p3, 4.0, &mut coarse);
        flatten_cubic(p0, p1, p2, p3, 0.25, &mut fine);
        assert!(
            fine.len() > coarse.len(),
            "tighter tolerance should add points: coarse={} fine={}",
            coarse.len(),
            fine.len(),
        );
    }

    #[test]
    fn degenerate_cubic_all_coincident() {
        // All control points identical — no curve, flattens to 2 coincident points.
        let p = Vec2::new(5.0, 7.0);
        let mut out = Vec::new();
        flatten_cubic(p, p, p, p, 0.25, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].p, p);
        assert_eq!(out[1].p, p);
    }
}
