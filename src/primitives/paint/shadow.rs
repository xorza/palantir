//! One drop or inset shadow behind a chrome or shape.

use crate::primitives::math::domain::{self, vec2};
use crate::primitives::math::nan::{self, NanCheck};
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;
use palantir_anim_derive::Animatable;

/// A single drop or inset shadow, embedded in `Shape::Shadow` or used as
/// `Background::shadow`. `Shadow::NONE` (also `Default`) is the "no shadow"
/// sentinel. `offset` is logical px; `blur` is the Gaussian σ (CSS
/// `blur-radius / 2`), 0 a sharp edge; `spread` inflates (drop) or deflates
/// (inset) the source rect. As in CSS a drop shadow is clipped inside the box
/// casting it.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, Animatable,
)]
#[must_use]
pub struct Shadow {
    /// Shadow colour.
    pub color: RgbaF32,
    /// Shift in logical pixels.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset2")]
    pub offset: Vec2,
    /// Gaussian σ in logical pixels, half CSS's `blur-radius`.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub blur: f32,
    /// Inflates a drop shadow's source rect, deflates an inset one's.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset")]
    pub spread: f32,
    /// Paints inside the chrome boundary rather than outside it.
    #[animate(snap)]
    pub inset: bool,
}

impl Shadow {
    /// Panics unless the colour is a [colour](domain::color), offset and spread
    /// are [offsets](domain::offset), and blur is a [length](domain::length).
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        let _ = domain::color(self.color);
        vec2::offset(self.offset);
        domain::length(self.blur);
        domain::offset(self.spread);
    }

    /// The "no shadow" sentinel; `const` for theme tables.
    pub const NONE: Self = Self {
        color: RgbaF32::TRANSPARENT,
        offset: Vec2::ZERO,
        blur: 0.0,
        spread: 0.0,
        inset: false,
    };

    /// A drop shadow of `color`, blurred by `blur` (σ) and shifted by `offset`, with no spread.
    pub const fn drop(color: RgbaF32, offset: Vec2, blur: f32) -> Self {
        Self {
            color,
            offset,
            blur,
            spread: 0.0,
            inset: false,
        }
    }

    /// Inflates (positive) or deflates (negative) the source rect by `spread` logical px.
    pub const fn with_spread(mut self, spread: f32) -> Self {
        self.spread = spread;
        self
    }

    /// Paints inside the shape boundary instead of outside it.
    pub const fn inset(mut self) -> Self {
        self.inset = true;
        self
    }

    /// `&self` because `Background` names this in a `skip_serializing_if`.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.color.is_noop() || self.has_nan()
    }

    /// True if any scalar is NaN; shared with the [`NanCheck`](crate::primitives::math::nan::NanCheck) impl.
    #[inline]
    pub(crate) const fn has_nan(&self) -> bool {
        self.color.has_nan()
            || nan::vec2_has_nan(self.offset)
            || self.blur.is_nan()
            || self.spread.is_nan()
    }
}

impl NanCheck for Shadow {
    #[inline]
    fn has_nan(&self) -> bool {
        Shadow::has_nan(self)
    }
}
