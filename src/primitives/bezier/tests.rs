use crate::primitives::bezier;
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
    let bbox = bezier::cubic_bbox(p0, p1, p2, p3);
    let (lo, hi) = (bbox.min, bbox.max());
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
    let bbox = bezier::cubic_bbox(p0, p1, p2, p3);
    let (lo, hi) = (bbox.min, bbox.max());
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
    let bbox = bezier::cubic_bbox(p0, p1, p2, p3);
    let (lo, hi) = (bbox.min, bbox.max());
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
