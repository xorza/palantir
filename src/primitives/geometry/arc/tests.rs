use crate::primitives::geometry::arc;
use crate::primitives::math::domain::internals::assert_close;
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const C: Vec2 = Vec2::new(10.0, 20.0);
const R: f32 = 5.0;

fn assert_bounds(a0: f32, a1: f32, lo: Vec2, hi: Vec2) {
    let b = arc::bbox(C, R, a0, a1);
    assert_eq!((b.min, b.max()), (lo, hi), "arc [{a0}, {a1}]");
}

/// Hand-computed bounds per sweep case; screen convention (0 = +x, π/2 = +y down).
#[test]
fn quarter_half_and_full_sweeps() {
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
    assert_bounds(0.0, TAU, C - Vec2::splat(R), C + Vec2::splat(R));
    // 3/4 sweep [0, 3π/2] (the spinner's arc): crosses +y and -x; the +x endpoint caps the right edge.
    assert_bounds(
        0.0,
        1.5 * PI,
        C - Vec2::splat(R),
        Vec2::new(C.x + R, C.y + R),
    );
}

/// A negative sweep traces its reversed twin; off-origin windows pick interior extremes.
#[test]
fn negative_sweep_and_offset_window() {
    let fwd = arc::bbox(C, R, -FRAC_PI_2, FRAC_PI_2);
    let rev = arc::bbox(C, R, FRAC_PI_2, -FRAC_PI_2);
    assert_eq!(fwd.min, rev.min);
    assert_eq!(fwd.max(), rev.max());
    assert_bounds(
        -FRAC_PI_2,
        FRAC_PI_2,
        Vec2::new(C.x, C.y - R),
        Vec2::new(C.x + R, C.y + R),
    );
    // Window [2π + π/4, 2π + 3π/4] crosses +y at 2π + π/2.
    let cos45 = 0.5f32.sqrt();
    let b = arc::bbox(C, R, TAU + 0.25 * PI, TAU + 0.75 * PI);
    let lo = Vec2::new(C.x - R * cos45, C.y + R * cos45);
    let hi = Vec2::new(C.x + R * cos45, C.y + R);
    let past_tau = "f32 cos of an angle past 2π carries an ulp of its own \
                    rounding, 4.8e-7 at the 6.46 it scales to";
    for (got, want) in [b.min.x, b.min.y, b.max().x, b.max().y]
        .into_iter()
        .zip([lo.x, lo.y, hi.x, hi.y])
    {
        assert_close(got, want, 1e-6, past_tau);
    }
}
