//! The model-tagged triple a picker drives, so no widget branches on the
//! model.

use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::hsv::Hsv;
use crate::primitives::paint::color::okhsv::Okhsv;

/// A picker's three axes together with the model they belong to.
///
/// The tag is the discriminant rather than a field beside a bare triple, so a
/// coordinate can never be read against the wrong model. Every widget drives
/// the axes through the accessors below and none of them matches on the
/// model.
///
/// A picker retains this between frames instead of re-deriving it from the
/// bound colour. Black has no hue and grey has no saturation, so a picker
/// that re-derived every frame would lose the hue the moment the value
/// reached zero.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub enum ColorCoords {
    /// Coordinates in [`ColorModel::Okhsv`].
    Okhsv(Okhsv),
    /// Coordinates in [`ColorModel::Hsv`].
    Hsv(Hsv),
}

/// Black in the default model — what a picker holds before its first frame
/// reads the bound colour.
impl Default for ColorCoords {
    fn default() -> Self {
        Self::Okhsv(Okhsv::default())
    }
}

impl ColorCoords {
    /// The axes of `color` in `model`.
    ///
    /// `fallback_hue` answers grey, which has no hue of its own.
    pub fn new(model: ColorModel, color: RgbaF32, fallback_hue: f32) -> Self {
        match model {
            ColorModel::Okhsv => Self::Okhsv(Okhsv::from_color(color, fallback_hue)),
            ColorModel::Hsv => Self::Hsv(Hsv::from_color(color, fallback_hue)),
        }
    }

    /// Which model these axes belong to.
    pub const fn model(self) -> ColorModel {
        match self {
            Self::Okhsv(_) => ColorModel::Okhsv,
            Self::Hsv(_) => ColorModel::Hsv,
        }
    }

    /// The opaque colour these axes name. A caller carrying alpha applies it
    /// with [`RgbaF32::with_alpha`].
    pub fn to_color(self) -> RgbaF32 {
        match self {
            Self::Okhsv(c) => c.to_color(),
            Self::Hsv(c) => c.to_color(),
        }
    }

    /// The same colour, read in `model` instead.
    ///
    /// Goes through [`Self::to_color`], so the colour survives the switch and
    /// only the handles move. Grey keeps its hue, because the current hue is
    /// what answers as the fallback.
    pub fn with_model(self, model: ColorModel) -> Self {
        if model == self.model() {
            return self;
        }
        Self::new(model, self.to_color(), self.hue())
    }

    /// Hue, read as a *fraction*: clamped to `0..=1`, and `0` for an axis
    /// the model holds as non-finite.
    pub const fn hue(self) -> f32 {
        domain::fraction(match self {
            Self::Okhsv(c) => c.h,
            Self::Hsv(c) => c.h,
        })
    }

    /// Saturation, read as a *fraction*: clamped to `0..=1`, and `0` for an axis
    /// the model holds as non-finite.
    pub const fn sat(self) -> f32 {
        domain::fraction(match self {
            Self::Okhsv(c) => c.s,
            Self::Hsv(c) => c.s,
        })
    }

    /// Value, read as a *fraction*: clamped to `0..=1`, and `0` for an axis
    /// the model holds as non-finite.
    pub const fn val(self) -> f32 {
        domain::fraction(match self {
            Self::Okhsv(c) => c.v,
            Self::Hsv(c) => c.v,
        })
    }

    /// Set the hue, as a *fraction* — clamped to `0..=1`, and `0` when it is
    /// not finite. Both ends name red, and both are kept: a hue bar dragged
    /// to its right edge reads 1 and draws its marker there, where a wrap
    /// to 0 jumped it to the left. A caller stepping round the circle wraps
    /// its own arithmetic.
    pub const fn set_hue(&mut self, h: f32) {
        let h = domain::fraction(h);
        match self {
            Self::Okhsv(c) => c.h = h,
            Self::Hsv(c) => c.h = h,
        }
    }

    /// Set the saturation, as a *fraction*.
    pub const fn set_sat(&mut self, s: f32) {
        let s = domain::fraction(s);
        match self {
            Self::Okhsv(c) => c.s = s,
            Self::Hsv(c) => c.s = s,
        }
    }

    /// Set the value, as a *fraction*.
    pub const fn set_val(&mut self, v: f32) {
        let v = domain::fraction(v);
        match self {
            Self::Okhsv(c) => c.v = v,
            Self::Hsv(c) => c.v = v,
        }
    }
}

#[cfg(test)]
mod tests;
