use crate::primitives::num::{F32Ext, Vec2Ext};
use glam::Vec2;

#[test]
fn fast_round_matches_std_round() {
    // Hand-picked halves pin the away-from-zero contract:
    // 0.5 → 1, 2.5 → 3 (not banker's 2), -0.5 → -1, -2.5 → -3.
    let cases: &[(f32, f32)] = &[
        (0.5, 1.0),
        (1.5, 2.0),
        (2.5, 3.0),
        (-0.5, -1.0),
        (-1.5, -2.0),
        (-2.5, -3.0),
        (0.49999997, 0.0),      // largest f32 below 0.5
        (0.50000006, 1.0),      // smallest f32 above 0.5
        (8388607.5, 8388608.0), // last representable half-step
        (-8388607.5, -8388608.0),
        (8388608.0, 8388608.0), // 2^23: fraction-free path
        (3.4e38, 3.4e38),
        (f32::INFINITY, f32::INFINITY),
        (f32::NEG_INFINITY, f32::NEG_INFINITY),
    ];
    for &(x, want) in cases {
        assert_eq!(x.fast_round().to_bits(), want.to_bits(), "x = {x}");
        assert_eq!(
            want.to_bits(),
            x.round().to_bits(),
            "case out of sync with std: {x}"
        );
    }
    assert!(f32::NAN.fast_round().is_nan());

    // Componentwise, and per axis: the two lanes carry different
    // cases (up vs away-from-zero down) so a swapped or duplicated
    // component fails.
    let v = Vec2::new(2.5, -0.5).fast_round();
    assert_eq!(v.x.to_bits(), 3.0f32.to_bits());
    assert_eq!(v.y.to_bits(), (-1.0f32).to_bits());

    // Dense sweep: bit-identical to `f32::round` across mixed
    // magnitudes and signs (0.0173 step avoids hitting only halves).
    for i in -60_000..60_000i32 {
        let x = i as f32 * 0.0173;
        assert_eq!(x.fast_round().to_bits(), x.round().to_bits(), "x = {x}");
    }
    // Exact half-integers across the i16 range.
    for i in -32_768..32_768i32 {
        let x = i as f32 + 0.5;
        assert_eq!(x.fast_round().to_bits(), x.round().to_bits(), "x = {x}");
    }
    // Signed-zero cases stay bit-exact: -0.0 and (-0.5, -0.0) → -0.0.
    assert_eq!((-0.0f32).fast_round().to_bits(), (-0.0f32).to_bits());
    assert_eq!((-0.25f32).fast_round().to_bits(), (-0.0f32).to_bits());
}

#[test]
fn is_integral_matches_round_equality() {
    let integral = [0.0, -0.0, 1.0, -7.0, 8388608.0, 1e18];
    let fractional = [0.1, -0.5, 1.5, 8388607.5, f32::NAN];
    for x in integral {
        assert!(x.is_integral(), "x = {x}");
        assert!(x == x.round(), "case out of sync with std: {x}");
    }
    for x in fractional {
        assert!(!x.is_integral(), "x = {x}");
        assert!(
            x != x.round() || x.is_nan(),
            "case out of sync with std: {x}"
        );
    }
    // Beyond i64 range the check conservatively reports false (the
    // equality it replaces said true); only forgoes a fast path.
    assert!(!1e30.is_integral());
    assert!(!f32::INFINITY.is_integral());
}

#[test]
fn quantize_px_snaps_to_whole_pixels_and_saturates() {
    // Half away from zero, matching `fast_round`, so the grid a cache
    // key lands on is the same one a wrap width lands on.
    for (v, expected) in [
        (0.0_f32, 0),
        (0.4, 0),
        (0.5, 1),
        (99.6, 100),
        (100.1, 100),
        (100.4, 100),
        (100.6, 101),
        (-0.4, 0),
        (-0.6, -1),
        (-100.5, -101),
    ] {
        assert_eq!(v.quantize_px(), expected, "v = {v}");
    }
    // Neighbouring inputs inside one pixel must collapse, adjacent
    // pixels must not — that collapse is what makes the key stable
    // under sub-pixel jitter during a resize drag.
    assert_eq!(100.1_f32.quantize_px(), 100.4_f32.quantize_px());
    assert_ne!(100.4_f32.quantize_px(), 100.6_f32.quantize_px());
    // An unbounded axis saturates instead of wrapping through `as`.
    assert_eq!(f32::INFINITY.quantize_px(), i32::MAX);
    assert_eq!(f32::NEG_INFINITY.quantize_px(), i32::MAX);
    assert_eq!(f32::NAN.quantize_px(), i32::MAX);
}
