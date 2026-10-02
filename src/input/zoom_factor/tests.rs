use crate::input::zoom_factor::{ZoomFactor, clamp, is_valid};

/// Every `f64` maps to a factor [`is_valid`] accepts — the property
/// the module's "every product goes through `clamp`" rests on.
///
/// Hand-computed: both saturating ends resolve to the `f32` extremes,
/// an ordinary product passes through unchanged, and NaN — which
/// neither comparison in `clamp` answers `true` for — resolves to
/// identity rather than falling through as NaN.
#[test]
fn clamp_maps_every_product_to_a_valid_factor() {
    let cases: &[(f64, f32)] = &[
        (f64::NEG_INFINITY, f32::MIN_POSITIVE),
        (-1.0, f32::MIN_POSITIVE),
        (0.0, f32::MIN_POSITIVE),
        (f64::from(f32::MIN_POSITIVE), f32::MIN_POSITIVE),
        (2.5, 2.5),
        (f64::from(f32::MAX), f32::MAX),
        (f64::INFINITY, f32::MAX),
        (f64::NAN, 1.0),
    ];
    for &(product, want) in cases {
        let got = clamp(product);
        assert_eq!(got, want, "clamp({product})");
        assert!(is_valid(got), "clamp({product}) left an invalid factor");
    }
}

/// `new` accepts exactly the factors that compose — a zoom cannot
/// invert, annihilate, or be a non-number.
#[test]
fn only_a_finite_positive_number_is_a_factor() {
    for good in [f32::MIN_POSITIVE, 0.5, 1.0, 2.0, f32::MAX] {
        assert!(ZoomFactor::new(good).is_some(), "{good} is a factor");
    }
    for bad in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(ZoomFactor::new(bad).is_none(), "{bad} is not a factor");
    }
}

/// **The reason the type exists.** A gesture is a running product, and
/// a naive `f32` one pushed far enough in one direction reaches zero
/// or infinity and cannot be composed back out. A thousand halvings
/// land on the smallest positive factor rather than zero, and a
/// thousand doublings walk all the way back.
#[test]
fn a_long_one_way_gesture_stays_invertible() {
    let half = ZoomFactor::new(0.5).unwrap();
    let double = ZoomFactor::new(2.0).unwrap();

    let mut zoom = ZoomFactor::ONE;
    for _ in 0..1000 {
        zoom = zoom.combine(half);
    }
    assert_eq!(zoom.get(), f32::MIN_POSITIVE, "clamped, not collapsed");
    assert!(ZoomFactor::new(zoom.get()).is_some());

    for _ in 0..1000 {
        zoom = zoom.combine(double);
    }
    assert_eq!(zoom.get(), f32::MAX, "clamped at the other end");
    assert!(ZoomFactor::new(zoom.get()).is_some());

    // Naively, the same walk is a one-way trip.
    let mut naive = 1.0_f32;
    for _ in 0..1000 {
        naive *= 0.5;
    }
    assert_eq!(naive, 0.0, "the trap this type removes");
    for _ in 0..1000 {
        naive *= 2.0;
    }
    assert_eq!(naive, 0.0, "and it never comes back");
}

/// Wheel-up is positive notches and zooms *in*, so the factor grows.
/// Hand-computed: `1.25^-1 = 0.8`, rounded to the nearest f32, and
/// `1.25^-(-2) = 1.5625`, exact in binary. Zero notches is the identity
/// whatever the step.
#[test]
fn wheel_notches_negate_into_the_factor() {
    assert_eq!(ZoomFactor::from_wheel(1.25, 1.0).get(), 0.8);
    assert_eq!(ZoomFactor::from_wheel(1.25, -2.0).get(), 1.5625);
    assert_eq!(ZoomFactor::from_wheel(1.25, 0.0), ZoomFactor::ONE);
}
