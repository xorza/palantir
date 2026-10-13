//! The scale the *user* chose, held apart from the scale the *system* reported.

use crate::primitives::math::domain::EPS;

/// A chrome scale the application chooses, multiplied onto the platform's;
/// `Display::scale_factor` is the product.
///
/// **A second factor, not one settable number:** the system factor is a fact
/// about the output and changes when the window moves monitors; the user factor
/// is a preference that survives that move. Keeping them apart makes "125%
/// everywhere" mean the same on both screens.
///
/// **Every distinct value costs a re-rasterization:** the effective factor
/// reaches the glyph cache key, so each new one mints fresh rasters and atlas
/// slots for every glyph on screen. Fine once, ruinous sixty times a second.
/// Hence [`Self::LADDER`] and the step methods; [`Self::new`] accepts any value
/// in range for apps that want a free one.
///
/// Not serializable on purpose: persist the `f32` from [`Self::get`] and read it
/// back through [`Self::new`] so the range check runs. A non-number comes back
/// as `None`; `UserScale::new(saved).unwrap_or_default()` falls back to
/// [`Self::ONE`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[must_use]
pub struct UserScale(f32);

impl Default for UserScale {
    fn default() -> Self {
        Self::ONE
    }
}

impl UserScale {
    /// No scaling.
    pub const ONE: Self = Self(1.0);

    /// The rungs [`Self::stepped_up`] and [`Self::stepped_down`] walk, and the
    /// range [`Self::new`] clamps into. The set a browser offers, denser near
    /// `1.0` where a coarse rung reads as a jump.
    pub const LADDER: [f32; 13] = [
        0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
    ];

    /// Smallest value [`Self::new`] yields.
    pub const MIN: f32 = Self::LADDER[0];

    /// Largest value [`Self::new`] yields.
    pub const MAX: f32 = Self::LADDER[Self::LADDER.len() - 1];

    /// `factor` clamped to [`Self::MIN`] ..= [`Self::MAX`], or `None` when not
    /// finite.
    ///
    /// Out-of-range values are clamped (a UI at 20x is not a UI); a non-finite
    /// factor is refused, as it would corrupt every pointer coordinate and logical
    /// size downstream.
    pub const fn new(factor: f32) -> Option<Self> {
        if factor.is_finite() {
            Some(Self(factor.clamp(Self::MIN, Self::MAX)))
        } else {
            None
        }
    }

    /// The scale factor.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// The effective factor: `system_scale` scaled by this one. The one definition
    /// of the product, so the host's per-event copy and a frame's
    /// [`Display`](crate::Display) can't disagree.
    #[inline]
    pub const fn applied_to(self, system_scale: f32) -> f32 {
        system_scale * self.0
    }

    /// The next rung above, or this value at [`Self::MAX`]. Strictly above, so a
    /// value between rungs steps to the higher.
    pub fn stepped_up(self) -> Self {
        match Self::LADDER.iter().find(|rung| **rung > self.0 + EPS) {
            Some(rung) => Self(*rung),
            None => self,
        }
    }

    /// The next rung below, or this value at [`Self::MIN`].
    pub fn stepped_down(self) -> Self {
        match Self::LADDER.iter().rev().find(|rung| **rung < self.0 - EPS) {
            Some(rung) => Self(*rung),
            None => self,
        }
    }

    /// The scale as a whole percent.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a user scale is a positive factor, so its percent is positive"
    )]
    pub const fn percent(self) -> u32 {
        (self.0 * 100.0).round() as u32
    }
}

#[cfg(test)]
mod tests;
