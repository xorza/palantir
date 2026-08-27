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
///
/// **Total over every `f64`**, which is what makes the module doc's "every
/// product goes through `clamp`" a guarantee rather than a property of the
/// callers. NaN needs its own arm because both comparisons below answer
/// `false` for it, so without one it falls through the `else` and comes
/// back out as NaN. It resolves to the identity factor: NaN is not a zoom
/// at any magnitude, and one that propagates poisons the running product
/// for the rest of the gesture — every later multiply against it is NaN
/// too, so no input the user could give would recover the view.
#[inline]
fn clamp(product: f64) -> f32 {
    if product.is_nan() {
        return 1.0;
    }
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

#[cfg(test)]
mod tests {
    use crate::input::zoom::{clamp, is_valid};

    /// Every `f64` maps to a factor [`is_valid`] accepts — the property
    /// the module's "every product goes through `clamp`" rests on.
    ///
    /// Hand-computed: both saturating ends resolve to the `f32` extremes,
    /// an ordinary product passes through unchanged, and NaN — which
    /// neither comparison in `clamp` answers `true` for — resolves to
    /// identity rather than falling through as NaN.
    #[test]
    fn clamp_maps_every_product_to_a_valid_factor() {
        let cases: &[(f64, f32)] = &[
            (f64::NEG_INFINITY, f32::MIN_POSITIVE),
            (-1.0, f32::MIN_POSITIVE),
            (0.0, f32::MIN_POSITIVE),
            (f64::from(f32::MIN_POSITIVE), f32::MIN_POSITIVE),
            (2.5, 2.5),
            (f64::from(f32::MAX), f32::MAX),
            (f64::INFINITY, f32::MAX),
            (f64::NAN, 1.0),
        ];
        for &(product, want) in cases {
            let got = clamp(product);
            assert_eq!(got, want, "clamp({product})");
            assert!(is_valid(got), "clamp({product}) left an invalid factor");
        }
    }
}
