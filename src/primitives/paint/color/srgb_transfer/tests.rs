use super::*;
use crate::primitives::math::domain::internals::assert_close;

/// The transfer function as the standard writes it, with `powf` in
/// `f64`, rounded to `f32` once.
fn reference(c: f64) -> f32 {
    if c <= 0.040_45 {
        (c / 12.92) as f32
    } else {
        ((c + 0.055) / 1.055).powf(2.4) as f32
    }
}

/// Every byte decodes to the reference bit for bit, through the table
/// and through the float path both, and encodes back to itself. The
/// float path takes the byte as the `f32` a caller would write,
/// `byte / 255`, so its reference does too.
#[test]
fn every_byte_decodes_exactly_and_encodes_back() {
    for byte in 0u8..=255 {
        let exact = reference(f64::from(byte) / 255.0);
        let decoded = DECODED_BYTES[usize::from(byte)];
        assert_eq!(decoded.to_bits(), exact.to_bits(), "table, byte {byte}");
        assert_eq!(encode_byte(decoded), byte, "round trip, byte {byte}");
        let c = f32::from(byte) / 255.0;
        assert_eq!(
            decode(f64::from(c)).to_bits(),
            reference(f64::from(c)).to_bits(),
            "float path, byte {byte}",
        );
    }
}

/// A dense sweep of `f32` inputs across the curved branch and on into
/// HDR values decodes to the reference bit for bit. Every 97th bit
/// pattern from the knee to 64, about 450 000 inputs.
#[test]
fn a_dense_float_sweep_decodes_exactly() {
    let first = 0.040_45f32.to_bits();
    let last = 64.0f32.to_bits();
    for bits in (first..=last).step_by(97) {
        let c = f64::from(f32::from_bits(bits));
        assert_eq!(decode(c).to_bits(), reference(c).to_bits(), "c = {c}");
    }
}

/// NaN and the infinities are not colours; they pass through rather
/// than turning into a finite value.
#[test]
fn non_finite_input_passes_through() {
    assert!(decode(f64::NAN).is_nan());
    assert_eq!(decode(f64::INFINITY), f32::INFINITY);
    assert_eq!(decode(f64::NEG_INFINITY), f32::NEG_INFINITY);
}

/// Each byte starts exactly at its threshold: the largest `f32` below
/// threshold `i` encodes to `i`, and the smallest at or above it to
/// `i + 1`. And the threshold is where the reference encode reaches
/// `i + ½` — to `1e-9` of a step, well past the `1e-13` an `f64`
/// `powf` round trip loses here.
#[test]
fn a_byte_rises_exactly_at_its_threshold() {
    for (i, &threshold) in BYTE_THRESHOLDS.iter().enumerate() {
        let rounded = threshold as f32;
        let at = if f64::from(rounded) < threshold {
            rounded.next_up()
        } else {
            rounded
        };
        let below = at.next_down();
        assert_eq!(usize::from(encode_byte(below)), i, "below threshold {i}");
        assert_eq!(usize::from(encode_byte(at)), i + 1, "at threshold {i}");
        let reached = if threshold <= 0.003_130_8 {
            threshold * 12.92
        } else {
            1.055 * threshold.powf(1.0 / 2.4) - 0.055
        } * 255.0;
        let midpoint = i as f64 + 0.5;
        assert_close(
            reached,
            midpoint,
            1e-9,
            "the reference encode reaches i + ½ to 1e-9 of a step",
        );
    }
}

/// Out of range saturates and NaN is zero, as `unit_to_u8` rounds.
#[test]
fn encode_byte_saturates_like_unit_to_u8() {
    let cases = [
        (f32::NAN, 0),
        (f32::NEG_INFINITY, 0),
        (-1.0, 0),
        (0.0, 0),
        (1.0, 255),
        (2.0, 255),
        (f32::INFINITY, 255),
    ];
    for (y, byte) in cases {
        assert_eq!(encode_byte(y), byte, "y = {y}");
    }
}

/// The byte by binary search over the `f64` thresholds: what
/// [`encode_byte`] computed before its bucket table, kept as its
/// reference.
fn searched(y: f32) -> u8 {
    let y = f64::from(y);
    BYTE_THRESHOLDS.partition_point(|&threshold| threshold <= y) as u8
}

/// The bucket lookup gives the searched byte for every 4099th `f32` bit
/// pattern from 0 to `+∞` — about half a million inputs across every
/// bucket — and for the values at its edges: NaN, the zeros, negatives,
/// the subnormals, the first bucket's start, and either side of `1.0`.
#[test]
fn encode_byte_matches_the_search() {
    let last = f32::INFINITY.to_bits();
    for bits in (0..=last).step_by(4099).chain([last]) {
        let y = f32::from_bits(bits);
        assert_eq!(encode_byte(y), searched(y), "y = {y:e}");
    }
    let edges = [
        f32::NAN,
        -0.0,
        -1.0,
        f32::NEG_INFINITY,
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        f32::from_bits(FIRST_BUCKET << BUCKET_SHIFT),
        THRESHOLDS_F32[0].next_down(),
        THRESHOLDS_F32[254],
        1.0f32.next_down(),
        1.0,
        f32::MAX,
    ];
    for y in edges {
        assert_eq!(encode_byte(y), searched(y), "y = {y:e}");
    }
}

/// Every non-negative `f32`, `+∞` included, gives the searched byte.
#[test]
#[ignore = "about 2·10⁹ inputs: run with --ignored after a change to the table"]
fn encode_byte_matches_the_search_everywhere() {
    for bits in 0..=f32::INFINITY.to_bits() {
        let y = f32::from_bits(bits);
        assert_eq!(encode_byte(y), searched(y), "y = {y:e}");
    }
}
