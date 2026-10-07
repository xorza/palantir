//! A line's colour and width, as a border or as a path's stroke.

use crate::primitives::math::domain;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use palantir_anim_derive::Animatable;

/// One colour and one width; where the width lies is the shape's rule. An area shape ([`Background`], rect, triangle) paints it as a border inside its edge; a path shape (line, curve, arc, polyline) centres it on the path.
///
/// [`Background`]: crate::Background
#[derive(
    Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, Animatable,
)]
pub struct Stroke {
    /// Ink colour.
    pub color: RgbaF32,
    /// Width in logical pixels.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub width: f32,
}

impl Stroke {
    /// Panics unless the width is a [length](domain::length) and the colour a [colour](domain::color).
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::length(self.width);
        let _ = domain::color(self.color);
    }

    /// The "no stroke" sentinel: width 0, transparent; `Stroke::default()` but `const`.
    pub const NONE: Self = Self {
        color: RgbaF32::TRANSPARENT,
        width: 0.0,
    };

    /// True when this stroke paints nothing: width within tolerance of zero (negative counts as zero) or fully transparent colour. Animation lerps through `Stroke::NONE`, so a bordered-to-borderless transition settles here. Takes `&self` because `Background`'s `skip_serializing_if` needs `fn(&T) -> bool`.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        is_invisible(self.width) || self.color.is_noop()
    }

    /// Construct a stroke with `color` and `width`.
    #[inline]
    pub const fn new(color: RgbaF32, width: f32) -> Self {
        Self { color, width }
    }
}

impl NanCheck for Stroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.color.has_nan() || self.width.is_nan()
    }
}
