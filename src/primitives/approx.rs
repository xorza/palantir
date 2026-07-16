use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::Vec2;
use std::hash::Hasher;

/// Float comparisons at UI tolerance.
///
/// `EPS = 1e-4` is below 8-bit color precision (1/255 ≈ 4e-3) and sub-pixel
/// position resolution at typical display scales, so differences smaller
/// than this are invisible to the user.
pub(crate) const EPS: f32 = 1.0e-4;

/// True if `c` is within `EPS` of zero.
#[inline]
pub(crate) const fn approx_zero(c: f32) -> bool {
    c.abs() <= EPS
}

/// Equality-compatible bits for public `Hash` implementations. Rust float
/// equality treats both signed zeros as equal, so they must share one hash;
/// every other value retains its exact representation.
#[inline]
pub(crate) const fn eq_bits(f: f32) -> u32 {
    if f == 0.0 { 0 } else { f.to_bits() }
}

/// Canonicalize an `f32` at visual content-cache boundaries: collapse values
/// visually indistinguishable from zero to one bit pattern and every NaN to
/// one quiet NaN. Values outside the zero tolerance retain their exact bits.
#[inline]
pub(crate) const fn canon_bits(f: f32) -> u32 {
    if f.is_nan() {
        f32::NAN.to_bits()
    } else if approx_zero(f) {
        0u32
    } else {
        f.to_bits()
    }
}

#[inline]
pub(crate) fn hash_f32<H: Hasher>(value: f32, state: &mut H) {
    state.write_u32(eq_bits(value));
}

#[inline]
pub(crate) fn hash_vec2<H: Hasher>(value: Vec2, state: &mut H) {
    state.write_u64(((eq_bits(value.x) as u64) << 32) | eq_bits(value.y) as u64);
}

#[inline]
pub(crate) fn hash_size<H: Hasher>(value: Size, state: &mut H) {
    state.write_u64(((eq_bits(value.w) as u64) << 32) | eq_bits(value.h) as u64);
}

#[inline]
pub(crate) fn hash_rect<H: Hasher>(value: Rect, state: &mut H) {
    hash_vec2(value.min, state);
    hash_size(value.size, state);
}

#[inline]
pub(crate) fn hash_visual_f32<H: Hasher>(value: f32, state: &mut H) {
    state.write_u32(canon_bits(value));
}

#[inline]
pub(crate) fn hash_visual_vec2<H: Hasher>(value: Vec2, state: &mut H) {
    state.write_u64(((canon_bits(value.x) as u64) << 32) | canon_bits(value.y) as u64);
}

#[inline]
pub(crate) fn hash_visual_size<H: Hasher>(value: Size, state: &mut H) {
    state.write_u64(((canon_bits(value.w) as u64) << 32) | canon_bits(value.h) as u64);
}

#[inline]
pub(crate) fn hash_visual_rect<H: Hasher>(value: Rect, state: &mut H) {
    hash_visual_vec2(value.min, state);
    hash_visual_size(value.size, state);
}

/// True if `v` would produce no visible paint when used as a
/// magnitude (stroke width, alpha, etc.). Captures three cases in
/// one comparison: `v <= EPS` is true for near-zero positives,
/// exact zero, and any negative; the `is_nan` branch handles the
/// NaN case (NaN compares false against everything). Useful as the
/// shared predicate behind `Stroke::is_noop`, `Color::is_noop`,
/// and per-variant `Shape::is_noop` checks — keeps the
/// "non-paintable scalar" contract in one place.
#[inline]
pub(crate) const fn noop_f32(v: f32) -> bool {
    v.is_nan() || v <= EPS
}

/// True if an f16 stored as `u16` bits is `≤ EPS` in absolute value.
/// Branch-free bit-pattern check — masks the sign bit and compares
/// directly against `EPS` as f16 bits, with no f16→f32 conversion.
/// Works because positive f16 values are monotonic in their bit
/// representation (IEEE 754 design). NaN's exponent bits land at
/// `0x7C00`+, well above the threshold, so NaN classifies as
/// non-zero — matches `Corners::approx_zero` semantics and treats
/// NaN as a loud programming bug rather than a silent skip.
#[inline]
pub(crate) const fn noop_f16_bits(bits: u16) -> bool {
    const EPS_BITS: u16 = half::f16::from_f32_const(EPS).to_bits();
    const ABS_MASK: u16 = 0x7FFF;
    (bits & ABS_MASK) <= EPS_BITS
}

/// True if an f16 stored as `u16` bits is within `EPS` below 1.0 (or
/// above). Mirror of `noop_f16_bits` for the opacity end of the
/// scale: positive f16 values are monotonic in their bit
/// representation, so `>= f16(1.0 - EPS).to_bits()` catches every
/// value visually indistinguishable from fully opaque. The upper
/// bound `< 0x7C00` rejects NaN (NaN exponent starts at `0x7C01`+)
/// — a NaN alpha is a loud bug, not a silent opaque pass. Negative
/// f16s carry the sign bit (`>= 0x8000`), well above the NaN
/// threshold, so they're rejected too.
#[inline]
pub(crate) const fn opaque_f16_bits(bits: u16) -> bool {
    const ONE_MINUS_EPS_BITS: u16 = half::f16::from_f32_const(1.0 - EPS).to_bits();
    const NAN_EXP: u16 = 0x7C00;
    bits >= ONE_MINUS_EPS_BITS && bits < NAN_EXP
}

/// True if two 2D points are within `EPS` of each other (Euclidean
/// distance). Compares squared distance against `EPS²` to avoid a
/// `sqrt`. Use when two points should be treated as coincident
/// (degenerate stroke endpoints, zero-length segments).
#[inline]
pub(crate) const fn vec2_approx_eq(a: glam::Vec2, b: glam::Vec2) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy <= EPS * EPS
}

#[cfg(test)]
mod tests {
    use crate::primitives::approx::{
        EPS, approx_zero, canon_bits, hash_rect, hash_visual_f32, hash_visual_rect,
    };
    use crate::primitives::rect::Rect;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher as _;

    fn finish_hash(write: impl FnOnce(&mut DefaultHasher)) -> u64 {
        let mut hasher = DefaultHasher::new();
        write(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn approx_zero_handles_boundary_sign_and_nan() {
        let cases: &[(&str, f32, bool)] = &[
            ("exact_zero", 0.0, true),
            ("neg_zero", -0.0, true),
            ("at_eps", EPS, true),
            ("at_neg_eps", -EPS, true),
            ("just_above_eps", EPS * 1.1, false),
            ("just_below_neg_eps", -EPS * 1.1, false),
            ("nan", f32::NAN, false),
        ];
        for (label, v, want) in cases {
            assert_eq!(approx_zero(*v), *want, "case: {label}");
        }
    }

    #[test]
    fn exact_hash_helpers_collapse_only_signed_zero() {
        let positive = Rect::new(0.0, 0.0, 0.0, 0.0);
        let negative = Rect::new(-0.0, -0.0, -0.0, -0.0);
        let sub_eps = Rect::new(EPS * 0.5, 0.0, 0.0, 0.0);

        assert_eq!(
            finish_hash(|h| hash_rect(positive, h)),
            finish_hash(|h| hash_rect(negative, h)),
        );
        assert_ne!(
            finish_hash(|h| hash_rect(positive, h)),
            finish_hash(|h| hash_rect(sub_eps, h)),
        );
    }

    #[test]
    fn visual_hash_helpers_collapse_zero_noise_and_nan_payloads() {
        let zero = Rect::ZERO;
        let sub_eps = Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS);
        assert_eq!(
            finish_hash(|h| hash_visual_rect(zero, h)),
            finish_hash(|h| hash_visual_rect(sub_eps, h)),
        );

        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        assert_eq!(canon_bits(nan_a), canon_bits(nan_b));
        assert_eq!(
            finish_hash(|h| hash_visual_f32(nan_a, h)),
            finish_hash(|h| hash_visual_f32(nan_b, h)),
        );
    }
}
