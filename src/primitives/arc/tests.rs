use crate::primitives::arc;
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const C: Vec2 = Vec2::new(10.0, 20.0);
const R: f32 = 5.0;

fn assert_bounds(a0: f32, a1: f32, lo: Vec2, hi: Vec2) {
    let b = arc::bbox(C, R, a0, a1);
    assert!(
        (b.min - lo).length() < 1e-4 && (b.max() - hi).length() < 1e-4,
        "arc [{a0}, {a1}]: got lo {:?} hi {:?}, want lo {lo:?} hi {hi:?}",
        b.min,
        b.max(),
    );
}

/// Hand-computed bounds per sweep case. Screen convention: angle 0
/// = +x, π/2 = +y (down). E.g. the quarter arc [0, π/2] traces
/// from (C.x + R, C.y) to (C.x, C.y + R), staying in the +x/+y
/// quadrant — bbox spans exactly those two endpoints.
#[test]
fn quarter_half_and_full_sweeps() {
    // Quarter [0, π/2]: endpoints only, no axis crossing inside.
    assert_bounds(
        0.0,
        FRAC_PI_2,
        Vec2::new(C.x, C.y),
        Vec2::new(C.x + R, C.y + R),
    );
    // Half [0, π]: crosses +y at π/2 → bbox reaches C.y + R.
    assert_bounds(
        0.0,
        PI,
        Vec2::new(C.x - R, C.y),
        Vec2::new(C.x + R, C.y + R),
    );
    // Full circle: center ± radius on both axes.
    assert_bounds(0.0, TAU, C - Vec2::splat(R), C + Vec2::splat(R));
    // 3/4 sweep [0, 3π/2] (the spinner's arc): crosses +y and -x,
    // misses only the top-right-of-(-y) quadrant gap; +x endpoint
    // caps the right edge.
    assert_bounds(
        0.0,
        1.5 * PI,
        C - Vec2::splat(R),
        Vec2::new(C.x + R, C.y + R),
    );
}

/// Negative sweep covers the same trace as its reversed positive
/// twin, and off-origin angle windows pick interior extremes.
#[test]
fn negative_sweep_and_offset_window() {
    // [π/2, -π/2] (negative direction) == [-π/2, π/2]: crosses +x.
    let fwd = arc::bbox(C, R, -FRAC_PI_2, FRAC_PI_2);
    let rev = arc::bbox(C, R, FRAC_PI_2, -FRAC_PI_2);
    assert!((fwd.min - rev.min).length() < 1e-6);
    assert!((fwd.max() - rev.max()).length() < 1e-6);
    assert_bounds(
        -FRAC_PI_2,
        FRAC_PI_2,
        Vec2::new(C.x, C.y - R),
        Vec2::new(C.x + R, C.y + R),
    );
    // Window far from 0: [2π + π/4, 2π + 3π/4] crosses +y at
    // 2π + π/2. Endpoints sit at ±R·cos(π/4) in x, +R·sin(π/4) in y.
    let cos45 = 0.5f32.sqrt();
    assert_bounds(
        TAU + 0.25 * PI,
        TAU + 0.75 * PI,
        Vec2::new(C.x - R * cos45, C.y + R * cos45),
        Vec2::new(C.x + R * cos45, C.y + R),
    );
}
