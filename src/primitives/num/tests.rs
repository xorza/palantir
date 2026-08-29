use crate::primitives::num::{F32Ext, Vec2Ext, unit_to_u8};
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

/// Hand-computed: `x * 255 + 0.5`, truncated. `0.5 → 127.5 + 0.5 = 128`,
/// `1/255 → 1.0 + 0.5 = 1.5 → 1`, and the largest f32 below 1.0 gives
/// `254.99998 + 0.5 = 255.49998 → 255`, so it saturates without ever
/// reaching 256.
#[test]
fn unit_to_u8_rounds_half_up() {
    let cases: &[(f32, u8)] = &[
        (0.0, 0),
        (-0.0, 0),
        (1.0 / 255.0, 1),
        (0.5, 128),
        (0.99999994, 255),
        (1.0, 255),
    ];
    for &(x, want) in cases {
        assert_eq!(unit_to_u8(x), want, "unit_to_u8({x})");
    }
}

/// The three cases the branch-free form leans on the saturating cast
/// for. Rust guarantees NaN → 0 and out-of-range → the nearest bound.
#[test]
fn unit_to_u8_saturates_instead_of_wrapping() {
    let cases: &[(f32, u8)] = &[
        (f32::NAN, 0),
        (-1.0e-3, 0),
        (-1.0e30, 0),
        (f32::NEG_INFINITY, 0),
        (1.0e30, 255),
        (f32::INFINITY, 255),
    ];
    for &(x, want) in cases {
        assert_eq!(unit_to_u8(x), want, "unit_to_u8({x})");
    }
}

/// Every byte survives a decode/encode round trip, which is what makes
/// the quantizer safe to apply to an already-quantized value.
#[test]
fn unit_to_u8_round_trips_every_byte() {
    for b in 0..=u8::MAX {
        assert_eq!(unit_to_u8(b as f32 / 255.0), b, "byte {b}");
    }
}

/// A 20 px band on a 120 px track leaves 100 px of travel, offset by
/// 10 px at each end: 10 → 0.0, 60 → 0.5, 110 → 1.0. Outside the track
/// the share runs past 0..1, which is the caller's to pin.
#[test]
fn band_fraction_offsets_by_half_the_band() {
    let cases: &[(f32, f32)] = &[
        (10.0, 0.0),
        (35.0, 0.25),
        (60.0, 0.5),
        (110.0, 1.0),
        (0.0, -0.1),
        (120.0, 1.1),
    ];
    for &(pos, want) in cases {
        let got = pos.band_fraction(120.0, 20.0);
        assert!(
            (got - want).abs() < 1e-6,
            "band_fraction({pos}) = {got}, want {want}"
        );
    }
}

/// A band at least as wide as its track leaves no travel, so there is no
/// share to report.
#[test]
fn band_fraction_reports_zero_without_travel() {
    assert_eq!(15.0_f32.band_fraction(20.0, 20.0), 0.0);
    assert_eq!(15.0_f32.band_fraction(10.0, 20.0), 0.0);
}

/// The screen every caller-supplied share passes: in-range values are
/// untouched, out-of-range ones clamp to the end they overshot, and a
/// value that names no share at all takes the caller's neutral rather
/// than an end.
///
/// The infinities matter as much as NaN — `f32::clamp` maps them to an
/// end, which states a share the caller never meant.
#[test]
fn unit_fraction_or_clamps_in_range_and_falls_back_outside_the_finite() {
    let cases: &[(f32, f32, f32)] = &[
        (0.0, 0.5, 0.0),
        (0.25, 0.5, 0.25),
        (1.0, 0.5, 1.0),
        (-0.3, 0.5, 0.0),
        (1.7, 0.5, 1.0),
        (f32::NAN, 0.5, 0.5),
        (f32::INFINITY, 0.5, 0.5),
        (f32::NEG_INFINITY, 0.5, 0.5),
        // The neutral is the caller's: the same non-finite input reads
        // as empty for a progress bar and as centred for a splitter.
        (f32::NAN, 0.0, 0.0),
        (f32::INFINITY, 1.0, 1.0),
    ];
    for &(value, fallback, want) in cases {
        assert_eq!(
            value.unit_fraction_or(fallback),
            want,
            "unit_fraction_or({value}, {fallback})",
        );
    }
}
