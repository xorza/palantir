//! The model-tagged axis triple a picker drives, so no widget branches on the model.

use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::{hsv, okhsv};

/// Hue, saturation and value in a [`ColorModel`], each `0..=1`: a picker's three axes, retained
/// between frames, since black has no hue and gray no saturation, so re-deriving from the bound
/// color would lose the hue. One triple for both models, which dispatch once per conversion.
/// Refused: a struct per model with the same three fields, and an enum of them whose every accessor
/// matched both arms to reach one field.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[must_use]
pub struct ColorCoords {
    model: ColorModel,
    h: f32,
    s: f32,
    v: f32,
}

impl ColorCoords {
    /// The axes `h`, `s`, `v` in `model`, read as [`Self::to_color`] paints them: a hue outside
    /// `0..=1` wraps, the others clamp, and a non-finite axis reads `0`.
    pub const fn new(model: ColorModel, h: f32, s: f32, v: f32) -> Self {
        let h = if domain::is_fraction(h) {
            h
        } else {
            domain::turn(h)
        };
        Self {
            model,
            h,
            s: domain::fraction(s),
            v: domain::fraction(v),
        }
    }

    /// Coordinates of `color` in `model`, ignoring its alpha; `fallback_hue` stands in where hue
    /// is undefined.
    pub fn from_color(color: RgbaF32, model: ColorModel, fallback_hue: f32) -> Self {
        match model {
            ColorModel::Okhsv => okhsv::from_color(color, fallback_hue),
            ColorModel::Hsv => hsv::from_color(color, fallback_hue),
        }
    }

    /// The color model.
    pub const fn model(self) -> ColorModel {
        self.model
    }

    /// The opaque color these coordinates give; apply alpha with [`RgbaF32::with_alpha`].
    pub fn to_color(self) -> RgbaF32 {
        self.model.slice(self.h).color(self.s, self.v)
    }

    /// The same color read in `model`; only the handles move, gray keeps its hue.
    pub fn with_model(self, model: ColorModel) -> Self {
        if model == self.model {
            return self;
        }
        Self::from_color(self.to_color(), model, self.h)
    }

    /// Hue, a turn in `0..=1`: both ends name red.
    pub const fn hue(self) -> f32 {
        self.h
    }

    /// Saturation, `0..=1`.
    pub const fn saturation(self) -> f32 {
        self.s
    }

    /// Value, `0..=1`.
    pub const fn value(self) -> f32 {
        self.v
    }

    /// These coordinates at hue `h`, a fraction: clamped to `0..=1`, `0` if non-finite. Both ends
    /// name red and both are kept, so a hue bar dragged to its right edge keeps its marker there.
    pub const fn with_hue(mut self, h: f32) -> Self {
        self.h = domain::fraction(h);
        self
    }

    /// These coordinates at saturation `s`, a fraction as [`Self::with_hue`] takes one.
    pub const fn with_saturation(mut self, s: f32) -> Self {
        self.s = domain::fraction(s);
        self
    }

    /// These coordinates at value `v`, a fraction as [`Self::with_hue`] takes one.
    pub const fn with_value(mut self, v: f32) -> Self {
        self.v = domain::fraction(v);
        self
    }
}

#[cfg(test)]
mod tests;
