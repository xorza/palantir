//! Zoom-factor arithmetic. Zoom is *multiplicative* — `1.0` is
//! identity, factors compose by multiplication — so accumulating one
//! over a gesture is a running product, and a long gesture in one
//! direction will otherwise underflow to `0.0` or overflow to `inf` and
//! never come back. Every product goes through [`clamp`] to keep the
//! running value invertible.
//!
//! Shared by the input state machine (accumulating `InputEvent::Zoom`
//! into a frame's pinch delta), the winit host (rejecting garbage
//! factors at ingress), and `widgets::scroll` (folding wheel notches and
//! pinch into one factor). Free functions rather than a newtype: the
//! value crosses the public API as a plain `f32` on `ScrollDelta::zoom`.

/// A valid factor is finite and strictly positive. Zero and negative
/// factors have no meaning (a zoom can't invert or annihilate) and
/// non-finite ones poison the running product.
#[inline]
pub(crate) fn is_valid(factor: f32) -> bool {
    factor.is_finite() && factor > 0.0
}

/// Bring a `f64` product back into the invertible `f32` range. Computed
/// in `f64` so the multiply itself can't lose the overshoot the clamp
/// needs to see.
#[inline]
fn clamp(product: f64) -> f32 {
    if product <= f64::from(f32::MIN_POSITIVE) {
        f32::MIN_POSITIVE
    } else if product >= f64::from(f32::MAX) {
        f32::MAX
    } else {
        product as f32
    }
}

/// Compose two factors.
#[inline]
pub(crate) fn combine(lhs: f32, rhs: f32) -> f32 {
    debug_assert!(is_valid(lhs));
    debug_assert!(!rhs.is_nan() && rhs >= 0.0);
    clamp(f64::from(lhs) * f64::from(rhs))
}

/// The factor `notches` of wheel travel represents, given a per-notch
/// `step`. Negated because wheel-up (positive notches) zooms *in*.
#[inline]
pub(crate) fn from_wheel(step: f32, notches: f32) -> f32 {
    debug_assert!(is_valid(step));
    debug_assert!(!notches.is_nan());
    clamp(f64::from(step.powf(-notches)))
}
