use crate::primitives::geometry::bezier::*;
use crate::primitives::math::approx::internals::assert_close;

#[test]
fn quadratic_to_cubic_promotes_inner_cps() {
    let p0 = Vec2::new(0.0, 0.0);
    let c = Vec2::new(50.0, 100.0);
    let p2 = Vec2::new(100.0, 0.0);
    let CubicControls { c1: q1, c2: q2 } = quadratic_to_cubic(p0, c, p2);
    // q1 = p0 + 2/3·(c − p0) = (100/3, 200/3), q2 = p2 + 2/3·(c − p2) =
    // (200/3, 200/3), each rounded once through the f32 2/3.
    let two_thirds = 2.0 / 3.0;
    assert_eq!(q1, p0 + (c - p0) * two_thirds);
    assert_eq!(q2, p2 + (c - p2) * two_thirds);
    assert_eq!(q1, Vec2::new(33.333336, 66.66667));
    assert_eq!(q2, Vec2::new(66.666664, 66.66667));
}

#[test]
fn cubic_bbox_is_endpoints_for_monotone_curve() {
    // Straight monotone curve along x: bbox = endpoint hull, no
    // contribution from inner CPs (which lie on the line).
    let p0 = Vec2::new(0.0, 0.0);
    let p1 = Vec2::new(33.0, 0.0);
    let p2 = Vec2::new(66.0, 0.0);
    let p3 = Vec2::new(100.0, 0.0);
    let bbox = cubic_bbox(p0, p1, p2, p3);
    assert_eq!((bbox.min, bbox.max()), (p0, p3));
}

/// S-curve: horizontal endpoints, inner CPs pulled vertically in opposite
/// directions, so the curve's excursion in y is far inside the control
/// hull's ±100.
///
/// Hand-computed: `y(t) = 300·t(1−t)(1−2t)`, whose derivative
/// `300(1 − 6t + 6t²)` vanishes at `t = (3 ∓ √3)/6`. There `u = t − ½ =
/// ∓√3/6`, and `y = 300·(¼ − u²)(−2u) = ±300·√3/18 = ±50/√3 ≈ ±28.87`. The
/// x extent is the endpoints', since x is monotone.
#[test]
fn cubic_bbox_tighter_than_control_hull_for_opposing_tangents() {
    let p0 = Vec2::new(0.0, 0.0);
    let p1 = Vec2::new(33.0, 100.0);
    let p2 = Vec2::new(66.0, -100.0);
    let p3 = Vec2::new(100.0, 0.0);
    let bbox = cubic_bbox(p0, p1, p2, p3);
    let (lo, hi) = (bbox.min, bbox.max());
    let extremum = 50.0 / 3.0f64.sqrt();
    assert_close(
        hi.y,
        extremum,
        1e-5,
        "the root goes through an f32 sqrt and a cubic evaluation, a few \
         ulps of 1.9e-6 at 28.9",
    );
    assert_eq!(lo.y, -hi.y, "the S is symmetric");
    assert_eq!((lo.x, hi.x), (0.0, 100.0), "the endpoints bound x");
}

#[test]
fn cubic_bbox_contains_sampled_curve() {
    // Stress: random-ish CPs; verify all sampled curve points lie
    // inside the reported bbox.
    let p0 = Vec2::new(10.0, 20.0);
    let p1 = Vec2::new(-30.0, 80.0);
    let p2 = Vec2::new(120.0, -40.0);
    let p3 = Vec2::new(90.0, 50.0);
    let bbox = cubic_bbox(p0, p1, p2, p3);
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
    assert_eq!(q_mid, c_mid);
}

/// Every quadratic, promoted to a cubic, keeps its extremum inside the
/// cubic bbox. The promotion's `2/3` blend leaves the cubic's `a`
/// coefficient at rounding residue rather than zero, which is where a
/// textbook root formula loses the small root.
///
/// The worked case: `p0 = 7, c = 49, p2 = 8` peaks at
/// `t* = (p0 − c)/(p0 − 2c + p2) = −42/−83 = 0.506`, where
/// `B(t*) = 7(1−t*)² + 98·t*(1−t*) + 8t*² = 28.253`.
#[test]
fn promoted_quadratics_keep_their_extremum_in_the_bbox() {
    let peak = |p0: f64, c: f64, p2: f64| {
        let t = (p0 - c) / (p0 - 2.0 * c + p2);
        let u = 1.0 - t;
        u * u * p0 + 2.0 * t * u * c + t * t * p2
    };
    let worked = |p0: f32, c: f32, p2: f32| {
        let (a, z) = (Vec2::new(0.0, p0), Vec2::new(1.0, p2));
        let ctl = quadratic_to_cubic(a, Vec2::new(0.5, c), z);
        cubic_bbox(a, ctl.c1, ctl.c2, z)
    };
    let bbox = worked(7.0, 49.0, 8.0);
    assert_close(
        bbox.max().y,
        peak(7.0, 49.0, 8.0),
        1e-3,
        "the f32 2/3 blend leaves the cubic a rounding residue off the \
         quadratic it promotes",
    );

    // A sweep over integer quadratics: every sampled point inside.
    let values: Vec<f32> = (0..800).step_by(53).map(|v| v as f32).collect();
    for &p0 in &values {
        for &c in &values {
            for &p2 in &values {
                let bbox = worked(p0, c, p2);
                for i in 0..=64 {
                    let t = i as f32 / 64.0;
                    let u = 1.0 - t;
                    let y = u * u * p0 + 2.0 * t * u * c + t * t * p2;
                    let slack = 1e-3 * y.abs().max(1.0);
                    assert!(
                        y >= bbox.min.y - slack && y <= bbox.max().y + slack,
                        "({p0}, {c}, {p2}) at t={t}: {y} outside {bbox:?}",
                    );
                }
            }
        }
    }
}
