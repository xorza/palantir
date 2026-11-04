//! The scale the *platform* reports, held apart from the scale the user chose.

use crate::display::user_scale::UserScale;
use crate::primitives::math::domain::EPS;

/// The device pixel ratio the platform reports for a window's output: physical pixels per
/// logical pixel, `2.0` on a 2× display. [`UserScale`] multiplies onto it, and
/// [`Display::scale_factor`](crate::Display::scale_factor) is the product.
///
/// **A fact, not a preference:** it changes when the window moves to a monitor of another
/// density, so it is checked and kept as the platform gives it, never clamped into a range as
/// the user's scale is. Its range is the product's divided by the user's, so any
/// [`UserScale`] on top keeps the product finite and at least `1e-4`: a
/// division by a product outside it corrupts every pointer coordinate and logical size
/// downstream.
///
/// Built through [`Self::new`], which answers `None` for the one rule it checks, as palantir takes
/// data from outside the program.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[must_use]
pub struct SystemScale(f32);

impl Default for SystemScale {
    fn default() -> Self {
        Self::ONE
    }
}

impl SystemScale {
    /// One physical pixel to a logical one.
    pub const ONE: Self = Self(1.0);

    /// The smallest scale, `2e-4`: at [`UserScale::MIN`] the product is `1e-4`.
    pub const MIN: f32 = EPS / UserScale::MIN;

    /// The largest scale: at [`UserScale::MAX`] the product is finite.
    pub const MAX: f32 = f32::MAX / UserScale::MAX;

    /// `factor` as a system scale, or `None` outside [`Self::MIN`] ..= [`Self::MAX`].
    pub const fn new(factor: f32) -> Option<Self> {
        if factor >= Self::MIN && factor <= Self::MAX {
            Some(Self(factor))
        } else {
            None
        }
    }

    /// The scale factor.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// The scale a windowing system reported, as an `f64` that promises nothing, or [`Self::ONE`]
    /// when it names no density: a bad report keeps the window usable, and is logged.
    #[cfg(feature = "winit")]
    pub(crate) fn from_platform(reported: f64) -> Self {
        Self::new(reported as f32).unwrap_or_else(|| {
            tracing::warn!(reported, "display.system_scale_rejected");
            Self::ONE
        })
    }
}

#[cfg(test)]
mod tests;
