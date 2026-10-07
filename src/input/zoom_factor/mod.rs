//! [`ZoomFactor`]: a multiplicative zoom that stays invertible.

/// A view zoom as a factor. [`Self::ONE`] is identity and factors compose by
/// multiplication; every product is clamped to stay invertible, as a naive
/// `f32` running product underflows or overflows and never recovers. An
/// **unbounded** view zoom should fold each frame's
/// [`ScrollDelta::zoom`](crate::ScrollDelta) in with [`Self::combine`].
///
/// ```
/// # use palantir::{ScrollDelta, ZoomFactor};
/// # fn on_frame(view: &mut ZoomFactor, scroll: ScrollDelta) {
/// *view = view.combine(scroll.zoom);
/// # }
/// ```
///
/// A zoom **bounded** by an authored range needs none of this; the bundled
/// [`Scroll`](crate::Scroll) clamps into its [`ZoomConfig`](crate::ZoomConfig).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[must_use]
pub struct ZoomFactor(f32);

impl Default for ZoomFactor {
    fn default() -> Self {
        Self::ONE
    }
}

impl ZoomFactor {
    /// No zoom; the identity of [`Self::combine`].
    pub const ONE: Self = Self(1.0);

    /// `factor` as a zoom, or `None` unless it is finite and strictly positive.
    #[inline]
    pub const fn new(factor: f32) -> Option<Self> {
        if is_valid(factor) {
            Some(Self(factor))
        } else {
            None
        }
    }

    /// The factor `notches` of wheel travel represents at a per-notch `step`; negated, as wheel-up zooms in.
    ///
    /// # Panics
    ///
    /// Panics unless `step` is a valid factor and `notches` is a number.
    #[inline]
    #[track_caller]
    pub fn from_wheel(step: f32, notches: f32) -> Self {
        assert!(is_valid(step), "a zoom step must be finite and positive");
        assert!(!notches.is_nan(), "wheel notches must be a number");
        Self(clamp(f64::from(step.powf(-notches))))
    }

    /// Composes with `rhs`, in `f64` so the clamp sees an overshoot; total.
    #[inline]
    pub fn combine(self, rhs: Self) -> Self {
        Self(clamp(f64::from(self.0) * f64::from(rhs.0)))
    }

    /// The factor as a plain number.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Finite and strictly positive.
#[inline]
const fn is_valid(factor: f32) -> bool {
    factor.is_finite() && factor > 0.0
}

/// Brings an `f64` product back into the invertible `f32` range. NaN needs its
/// own arm (both comparisons are `false`) and resolves to identity.
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

#[cfg(test)]
mod tests;
