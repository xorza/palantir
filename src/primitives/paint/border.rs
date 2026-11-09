//! [`Border`]: a box's edge, its color and width.

use crate::primitives::math::animatable::PX_SETTLE_EPS;
use crate::primitives::math::domain;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use palantir_derive::{Animatable, Lerp};

/// A box's edge: one color and one width, painted inside the box's boundary, as CSS's `border`
/// is. A [`Paint`](crate::Paint) holds one for a rect, a circle and a triangle, and a node's
/// [`Chrome`](crate::Chrome) through its paint; a line takes a [`Stroke`](crate::Stroke).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, Lerp, Animatable,
)]
#[animate(coerce = Self::coerce)]
pub struct Border {
    /// Ink color.
    pub color: RgbaF32,
    /// Width in logical pixels.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    #[animate(settle_eps = PX_SETTLE_EPS)]
    pub width: f32,
}

impl Border {
    /// Whether the width is a length and the color a color: [`Self::validate`]'s test.
    #[inline]
    pub(crate) const fn is_valid(&self) -> bool {
        domain::is_length(self.width) & domain::is_color(self.color)
    }

    /// Panics unless the width is a [length](domain::length) and the color a [color](domain::color).
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::length(self.width);
        let _ = domain::color(self.color);
    }

    /// Coerce the width to a length, as a curve overshooting below zero leaves it, as
    /// [`Shadow`](crate::Shadow)'s blur is.
    const fn coerce(&mut self) {
        self.width = domain::raised_to(self.width, 0.0);
    }

    /// The "no border" sentinel: width 0, transparent; `Border::default()` but `const`.
    pub const NONE: Self = Self {
        color: RgbaF32::TRANSPARENT,
        width: 0.0,
    };

    /// True when this border paints nothing: width within tolerance of zero (negative counts as
    /// zero) or fully transparent color. Animation lerps through `Border::NONE`, so a
    /// bordered-to-borderless transition settles here. Takes `&self` because `Paint`'s
    /// `skip_serializing_if` needs `fn(&T) -> bool`.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        is_invisible(self.width) || self.color.is_noop()
    }

    /// A border of `color` and `width`.
    #[inline]
    pub const fn new(color: RgbaF32, width: f32) -> Self {
        Self { color, width }
    }
}

impl NanCheck for Border {
    #[inline]
    fn has_nan(&self) -> bool {
        self.color.has_nan() || self.width.is_nan()
    }
}
