/// Float comparisons at UI tolerance.
///
/// `EPS = 1e-4` is below 8-bit color precision (1/255 ≈ 4e-3) and sub-pixel
/// position resolution at typical display scales, so differences smaller
/// than this are invisible to the user.
pub(crate) const EPS: f32 = 1.0e-4;

/// True if `c` is within `EPS` of zero. Const-friendly via plain
/// comparisons (`f32::abs` is not const-stable).
///
/// Was previously a trait (`ApproxF32`) with `approx_zero` + `approx_eq`
/// impls for `f32`/`Vec2`/`Size`, but only `f32::approx_zero` ever had a
/// caller; the rest was dead weight. If `approx_eq` or other-typed
/// `approx_zero` is ever needed, add focused free fns here, not a trait
/// (trait method `const` requires nightly).
#[inline]
pub const fn approx_zero(c: f32) -> bool {
    c >= -EPS && c <= EPS
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
pub const fn noop_f32(v: f32) -> bool {
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
