/// Float comparisons at UI tolerance.
///
/// `EPS = 1e-4` is below 8-bit color precision (1/255 ≈ 4e-3) and sub-pixel
/// position resolution at typical display scales, so differences smaller
/// than this are invisible to the user.
const EPS: f32 = 1.0e-4;

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
