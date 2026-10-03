//! [`ZoomFactor`] — a multiplicative zoom that stays invertible.

/// A view zoom, as a factor rather than a level.
///
/// **Multiplicative.** [`Self::ONE`] is identity, factors compose by
/// multiplication, and a gesture accumulates a running product. That is
/// what makes the newtype earn its place: a product taken naively in
/// `f32` and pushed far enough in one direction underflows to zero or
/// overflows to infinity, and never comes back. Every product here goes
/// through a clamp that keeps the value invertible, so a canvas cannot
/// be scrolled into a state it can never leave.
///
/// An **unbounded** view zoom holds one of these and folds each frame's
/// [`ScrollDelta::zoom`](crate::ScrollDelta) into it with
/// [`Self::combine`]. Storing the `f32` from [`Self::get`] instead and
/// multiplying by hand is the mistake this type exists to make
/// unavailable.
///
/// ```
/// # use palantir::{ScrollDelta, ZoomFactor};
/// # fn on_frame(view: &mut ZoomFactor, scroll: ScrollDelta) {
/// *view = view.combine(scroll.zoom);
/// # }
/// ```
///
/// A zoom **bounded** by an authored range needs none of this, and the
/// bundled [`Scroll`](crate::Scroll) is one: it clamps every step into
/// its [`ZoomConfig`](crate::ZoomConfig) range, which is finite and
/// positive, so its product cannot escape either. Reach for this type
/// where there is no such range — a canvas the user zooms as far as they
/// like.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct ZoomFactor(f32);

impl Default for ZoomFactor {
    fn default() -> Self {
        Self::ONE
    }
}

impl ZoomFactor {
    /// No zoom. The identity of [`Self::combine`].
    pub const ONE: Self = Self(1.0);

    /// `factor` as a zoom, or `None` when it is not one.
    ///
    /// A valid factor is finite and strictly positive. Zero and negative
    /// factors have no meaning — a zoom cannot invert or annihilate —
    /// and a non-finite one poisons every product it enters.
    #[inline]
    pub fn new(factor: f32) -> Option<Self> {
        is_valid(factor).then_some(Self(factor))
    }

    /// The factor `notches` of wheel travel represents, given a
    /// per-notch `step`.
    ///
    /// Negated because wheel-up — positive notches — zooms *in*.
    ///
    /// # Panics
    ///
    /// Panics unless `step` is a valid factor and `notches` is a number.
    /// A cold call on a wheel event, so the check costs a frame nothing.
    #[inline]
    pub fn from_wheel(step: f32, notches: f32) -> Self {
        assert!(is_valid(step), "a zoom step must be finite and positive");
        assert!(!notches.is_nan(), "wheel notches must be a number");
        Self(clamp(f64::from(step.powf(-notches))))
    }

    /// Compose with `rhs` — the accumulate step.
    ///
    /// The product is taken in `f64` so the clamp can see an overshoot
    /// the `f32` multiply would already have lost, then brought back into
    /// the invertible range. Composing is therefore total: no sequence of
    /// valid factors reaches a value that cannot be composed again.
    #[inline]
    pub fn combine(self, rhs: Self) -> Self {
        Self(clamp(f64::from(self.0) * f64::from(rhs.0)))
    }

    /// The factor as a plain number, for the transform that applies it.
    #[inline]
    pub fn get(self) -> f32 {
        self.0
    }
}

/// A valid factor is finite and strictly positive.
#[inline]
fn is_valid(factor: f32) -> bool {
    factor.is_finite() && factor > 0.0
}

/// Bring a `f64` product back into the invertible `f32` range. Computed
/// in `f64` so the multiply itself cannot lose the overshoot the clamp
/// needs to see.
///
/// **Total over every `f64`**, which is what makes the type's "every
/// product stays invertible" a guarantee rather than a property of the
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

#[cfg(test)]
mod tests;
