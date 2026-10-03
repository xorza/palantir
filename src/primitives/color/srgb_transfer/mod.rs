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
mod tests;
