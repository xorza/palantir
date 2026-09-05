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
mod tests {
    use crate::input::zoom_factor::{ZoomFactor, clamp, is_valid};

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

    /// `new` accepts exactly the factors that compose — a zoom cannot
    /// invert, annihilate, or be a non-number.
    #[test]
    fn only_a_finite_positive_number_is_a_factor() {
        for good in [f32::MIN_POSITIVE, 0.5, 1.0, 2.0, f32::MAX] {
            assert!(ZoomFactor::new(good).is_some(), "{good} is a factor");
        }
        for bad in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(ZoomFactor::new(bad).is_none(), "{bad} is not a factor");
        }
    }

    /// **The reason the type exists.** A gesture is a running product, and
    /// a naive `f32` one pushed far enough in one direction reaches zero
    /// or infinity and cannot be composed back out. A thousand halvings
    /// land on the smallest positive factor rather than zero, and a
    /// thousand doublings walk all the way back.
    #[test]
    fn a_long_one_way_gesture_stays_invertible() {
        let half = ZoomFactor::new(0.5).unwrap();
        let double = ZoomFactor::new(2.0).unwrap();

        let mut zoom = ZoomFactor::ONE;
        for _ in 0..1000 {
            zoom = zoom.combine(half);
        }
        assert_eq!(zoom.get(), f32::MIN_POSITIVE, "clamped, not collapsed");
        assert!(ZoomFactor::new(zoom.get()).is_some());

        for _ in 0..1000 {
            zoom = zoom.combine(double);
        }
        assert_eq!(zoom.get(), f32::MAX, "clamped at the other end");
        assert!(ZoomFactor::new(zoom.get()).is_some());

        // Naively, the same walk is a one-way trip.
        let mut naive = 1.0_f32;
        for _ in 0..1000 {
            naive *= 0.5;
        }
        assert_eq!(naive, 0.0, "the trap this type removes");
        for _ in 0..1000 {
            naive *= 2.0;
        }
        assert_eq!(naive, 0.0, "and it never comes back");
    }

    /// Wheel-up is positive notches and zooms *in*, so the factor grows.
    /// Hand-computed: `1.25^-(-2) = 1.5625`, and zero notches is the
    /// identity whatever the step.
    #[test]
    fn wheel_notches_negate_into_the_factor() {
        assert!((ZoomFactor::from_wheel(1.25, 1.0).get() - 0.8).abs() < 1e-6);
        assert!((ZoomFactor::from_wheel(1.25, -2.0).get() - 1.5625).abs() < 1e-5);
        assert_eq!(ZoomFactor::from_wheel(1.25, 0.0), ZoomFactor::ONE);
    }
}
