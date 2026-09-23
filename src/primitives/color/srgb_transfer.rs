//! The sRGB transfer function of IEC 61966-2-1, both ways, exact.
//!
//! Exact because the GPU applies the exact function at every sRGB write,
//! so an approximation here shows on screen: near black one display step
//! is a few thousandths in linear light, and a cubic fit read `#0a0a0a`
//! back as `#050505`.
//!
//! Fast as well, because colours are authored inside the record closure:
//! an `RgbaF32::srgb` per shape per frame is ordinary code, and the
//! decode sits on that path.

/// sRGB → linear, rounded to `f32` once from the `f64` evaluation.
///
/// `const`, because [`RgbaF32::srgb`](crate::RgbaF32::srgb) builds
/// constants and `powf` is not a `const fn`.
pub(super) const fn decode(c: f64) -> f32 {
    decode_f64(c) as f32
}

/// [`decode`] of every sRGB byte, `byte / 255`, evaluated at compile
/// time, so a hex colour decodes by lookup.
pub(super) const DECODED_BYTES: [f32; 256] = {
    let mut table = [0.0; 256];
    let mut byte = 0;
    while byte < 256 {
        table[byte] = decode(byte as f64 / 255.0);
        byte += 1;
    }
    table
};

/// Linear → sRGB, the inverse of [`decode`], in `f64` and rounded once.
/// For a caller that needs the encoded value itself; a byte comes from
/// [`encode_byte`].
pub(super) fn encode(y: f32) -> f32 {
    let y = f64::from(y);
    let c = if y <= 0.003_130_8 {
        y * 12.92
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    };
    c as f32
}

/// The sRGB byte `y` encodes to: `255 · encode(y)` rounded half up and
/// saturated, with NaN at 0 — how `num::unit_to_u8` rounds.
///
/// **Counted, not evaluated.** The byte rises to `i + 1` exactly where
/// `y` reaches the decode of the midpoint `(i + ½) / 255`, so counting
/// the midpoints `y` has reached is exact and costs a binary search,
/// where evaluating [`encode`] costs a `powf` and is only as exact as it.
pub(super) fn encode_byte(y: f32) -> u8 {
    let y = f64::from(y);
    BYTE_THRESHOLDS.partition_point(|&threshold| threshold <= y) as u8
}

/// Entry `i` is the linear value at which the encode reaches `(i + ½) /
/// 255` — see [`encode_byte`].
const BYTE_THRESHOLDS: [f64; 255] = {
    let mut thresholds = [0.0; 255];
    let mut i = 0;
    while i < 255 {
        thresholds[i] = decode_f64((i as f64 + 0.5) / 255.0);
        i += 1;
    }
    thresholds
};

/// [`decode`] before its rounding to `f32`, which the thresholds need.
///
/// `x^2.4` is taken as `x⁴ · x^(-8/5)`, the second factor from
/// [`inverse_fifth_root`].
const fn decode_f64(c: f64) -> f64 {
    if c <= 0.040_45 {
        return c / 12.92;
    }
    // NaN and +∞ pass through: neither has a power that is a colour.
    if c.is_nan() || c == f64::INFINITY {
        return c;
    }
    let x = (c + 0.055) * (1.0 / 1.055);
    let y = inverse_fifth_root(x);
    let x2 = x * x;
    let y2 = y * y;
    let y4 = y2 * y2;
    x2 * x2 * (y4 * y4)
}

/// `x^(-1/5)` for a normal, finite, positive `x`.
///
/// Range reduction first: `x = 2^(b − 1023) · m` with `m` in `[1, 2)`,
/// and the biased exponent `b = 5q + k` with `k` in `0..5`. `1023 / 5` is
/// `204.6`, so the root is `2^(204 − q) · 2^(0.6 − k/5) · m^(-1/5)`. A
/// degree-5 Chebyshev fit of `m^(-1/5)` on `[1, 2)` seeds it within
/// `6.4e-6`.
///
/// Then Newton on the inverse root, `y ← y · (6 − x·y⁵) / 5`, which takes
/// a relative error `ε` to about `3ε²` and, unlike the step for the root
/// itself, divides by nothing. Two steps reach `4e-20`, past `f64`
/// precision.
const fn inverse_fifth_root(x: f64) -> f64 {
    const MANTISSA: u64 = (1 << 52) - 1;
    const ONE_BITS: u64 = 1.0f64.to_bits();
    /// `2^(0.6 − k/5)` for `k` in `0..5`.
    const STEP: [f64; 5] = [
        1.515_716_566_510_398,
        1.319_507_910_772_894_2,
        1.148_698_354_997_034_9,
        1.0,
        0.870_550_563_296_124_1,
    ];
    /// The fit of `m^(-1/5)`, lowest power first.
    const FIT: [f64; 6] = [
        1.433_378_063_170_971_6,
        -0.835_564_291_520_950_9,
        0.630_674_330_615_126_8,
        -0.296_770_276_217_870_1,
        0.076_566_500_701_655_58,
        -0.008_290_707_072_299_919,
    ];

    let bits = x.to_bits();
    let biased = (bits >> 52) & 0x7ff;
    let m = f64::from_bits((bits & MANTISSA) | ONE_BITS);
    let q = biased / 5;
    let k = (biased % 5) as usize;

    let mut fit = FIT[5];
    let mut power = 5;
    while power > 0 {
        power -= 1;
        fit = fit * m + FIT[power];
    }
    let mut y = fit * STEP[k] * f64::from_bits((1227 - q) << 52);

    let mut step = 0;
    while step < 2 {
        let y2 = y * y;
        y = y * (6.0 - x * (y2 * y2 * y)) * 0.2;
        step += 1;
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(
                (reached - midpoint).abs() < 1e-9,
                "threshold {i} reaches {reached}, not {midpoint}",
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
}
