use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::internals::assert_close;
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const C: Vec2 = Vec2::new(10.0, 20.0);
const R: f32 = 5.0;

fn bbox(start_angle: f32, sweep: f32) -> Rect {
    CircularArc {
        center: C,
        radius: R,
        start_angle,
        sweep,
    }
    .bbox()
}

fn assert_bounds(start_angle: f32, sweep: f32, lo: Vec2, hi: Vec2) {
    let b = bbox(start_angle, sweep);
    assert_eq!((b.min, b.max()), (lo, hi), "arc {sweep} from {start_angle}");
}

/// A sweep within one full circle stays as authored, its ends included; past one, it clamps to `±2π` with its sign.
#[test]
fn coercion_clamps_the_sweep_to_one_full_circle() {
    let cases = [
        (FRAC_PI_2, FRAC_PI_2),
        (-PI, -PI),
        (TAU, TAU),
        (-TAU, -TAU),
        (3.0 * PI, TAU),
        (-5.0 * PI, -TAU),
        (f32::MAX, TAU),
    ];
    for (sweep, want) in cases {
        let arc = CircularArc {
            center: C,
            radius: R,
            start_angle: 0.25,
            sweep,
        };
        assert_eq!(
            arc.coerced(),
            CircularArc { sweep: want, ..arc },
            "sweep {sweep}"
        );
    }
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
    let fwd = bbox(-FRAC_PI_2, PI);
    let rev = bbox(FRAC_PI_2, -PI);
    assert_eq!(fwd.min, rev.min);
    assert_eq!(fwd.max(), rev.max());
    assert_bounds(
        -FRAC_PI_2,
        PI,
        Vec2::new(C.x, C.y - R),
        Vec2::new(C.x + R, C.y + R),
    );
    // Window [2π + π/4, 2π + 3π/4] crosses +y at 2π + π/2.
    let cos45 = 0.5f32.sqrt();
    let b = bbox(TAU + 0.25 * PI, FRAC_PI_2);
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
