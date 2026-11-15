//! [`PaintCurve`]: the curve a paint-time animation runs its phase through.

use std::f32::consts::TAU;

use crate::animation::easing::Easing;
use crate::primitives::math::libm_ext::Libm;

/// A phase in `[0, 1)` mapped to where the channel sits: `0` at its range's `from`, `1` at its
/// `to`. A value may leave `[0, 1]`, as [`Easing::OutBack`]'s overshoot does: a turn then swings
/// past its range, and an alpha clamps.
///
/// The shipped curves are its constants, a one-way ease converts from an [`Easing`], and any
/// other is [`Self::new`] over a plain `fn`, as Flutter's `Curves` holds the curves it ships.
/// A plain `fn`, so it cannot capture: the encoder samples with no accumulator, so a dropped
/// frame or irregular `dt` must not make it drift. No `PartialEq`: a `fn` compares by address.
#[derive(Clone, Copy, Debug, Hash)]
pub struct PaintCurve(fn(f32) -> f32);

impl PaintCurve {
    /// The phase itself, the default.
    pub const LINEAR: Self = Self(|t| t);

    /// A raised cosine, zero at both ends; the zero slope leaves no seam when repeating.
    pub const SINE: Self = Self(|t| (1.0 - (t * TAU).libm_cos()) * 0.5);

    /// One for the first half of the period, zero for the second, as a caret blinks; pair with
    /// [`PaintAnimation::with_steps(2)`](crate::PaintAnimation::with_steps).
    pub const SQUARE: Self = Self(|t| if t < 0.5 { 1.0 } else { 0.0 });

    /// A curve of your own.
    pub const fn new(curve: fn(f32) -> f32) -> Self {
        Self(curve)
    }

    /// Where `phase` lands on the curve.
    #[inline]
    pub fn apply(self, phase: f32) -> f32 {
        (self.0)(phase)
    }
}

/// The ease a [`Ui::animate`](crate::Ui::animate) tween follows, run once over each pass of the
/// paint animation.
impl From<Easing> for PaintCurve {
    fn from(easing: Easing) -> Self {
        match easing {
            Easing::Linear => Self::LINEAR,
            Easing::OutCubic => Self(|t| Easing::OutCubic.apply(t)),
            Easing::InOutCubic => Self(|t| Easing::InOutCubic.apply(t)),
            Easing::OutQuart => Self(|t| Easing::OutQuart.apply(t)),
            Easing::OutBack => Self(|t| Easing::OutBack.apply(t)),
        }
    }
}
