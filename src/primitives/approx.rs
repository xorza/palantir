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
pub const fn noop_f32(v: f32) -> bool {
    v.is_nan() || v <= EPS
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
