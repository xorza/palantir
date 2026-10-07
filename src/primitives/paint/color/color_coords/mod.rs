//! The model-tagged axis triple a picker drives, so no widget branches on the model.

use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::hsv::Hsv;
use crate::primitives::paint::color::okhsv::Okhsv;

/// A picker's three axes with their model, retained between frames: black has no hue and grey no
/// saturation, so re-deriving from the bound colour would lose the hue.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub enum ColorCoords {
    /// Okhsv coordinates.
    Okhsv(Okhsv),
    /// HSV coordinates.
    Hsv(Hsv),
}

impl Default for ColorCoords {
    fn default() -> Self {
        Self::Okhsv(Okhsv::default())
    }
}

impl ColorCoords {
    /// Coordinates of `color` in `model`; `fallback_hue` stands in where hue is undefined.
    pub fn new(model: ColorModel, color: RgbaF32, fallback_hue: f32) -> Self {
        match model {
            ColorModel::Okhsv => Self::Okhsv(Okhsv::from_color(color, fallback_hue)),
            ColorModel::Hsv => Self::Hsv(Hsv::from_color(color, fallback_hue)),
        }
    }

    /// The colour model.
    pub const fn model(self) -> ColorModel {
        match self {
            Self::Okhsv(_) => ColorModel::Okhsv,
            Self::Hsv(_) => ColorModel::Hsv,
        }
    }

    /// The colour these coordinates give.
    pub fn to_color(self) -> RgbaF32 {
        match self {
            Self::Okhsv(c) => c.to_color(),
            Self::Hsv(c) => c.to_color(),
        }
    }

    /// The same colour read in `model`; only the handles move, grey keeps its hue.
    pub fn with_model(self, model: ColorModel) -> Self {
        if model == self.model() {
            return self;
        }
        Self::new(model, self.to_color(), self.hue())
    }

    /// Hue as the turn [`Self::to_color`] paints: wraps outside `0..=1`, non-finite reads `0`.
    pub const fn hue(self) -> f32 {
        let h = match self {
            Self::Okhsv(c) => c.h,
            Self::Hsv(c) => c.h,
        };
        if domain::is_fraction(h) {
            h
        } else {
            domain::turn(h)
        }
    }

    /// Clamped to `0..=1`, `0` if non-finite.
    pub const fn saturation(self) -> f32 {
        domain::fraction(match self {
            Self::Okhsv(c) => c.s,
            Self::Hsv(c) => c.s,
        })
    }

    /// Clamped to `0..=1`, `0` if non-finite.
    pub const fn value(self) -> f32 {
        domain::fraction(match self {
            Self::Okhsv(c) => c.v,
            Self::Hsv(c) => c.v,
        })
    }

    /// Sets the hue as a fraction: clamped to `0..=1`, `0` if non-finite. Both ends name red and
    /// both are kept, so a hue bar dragged to its right edge keeps its marker there.
    pub const fn set_hue(&mut self, h: f32) {
        let h = domain::fraction(h);
        match self {
            Self::Okhsv(c) => c.h = h,
            Self::Hsv(c) => c.h = h,
        }
    }

    /// Sets saturation.
    pub const fn set_saturation(&mut self, s: f32) {
        let s = domain::fraction(s);
        match self {
            Self::Okhsv(c) => c.s = s,
            Self::Hsv(c) => c.s = s,
        }
    }

    /// Sets value.
    pub const fn set_value(&mut self, v: f32) {
        let v = domain::fraction(v);
        match self {
            Self::Okhsv(c) => c.v = v,
            Self::Hsv(c) => c.v = v,
        }
    }
}

#[cfg(test)]
mod tests;
