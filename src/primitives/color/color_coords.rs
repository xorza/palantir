//! The model-tagged triple a picker drives, so no widget branches on the
//! model.

use crate::primitives::color::RgbaF32;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::color::hsv::Hsv;
use crate::primitives::color::okhsv::Okhsv;

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
pub enum ColorCoords {
    Okhsv(Okhsv),
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

    /// Hue, `0..1`.
    pub const fn hue(self) -> f32 {
        match self {
            Self::Okhsv(c) => c.h,
            Self::Hsv(c) => c.h,
        }
    }

    /// Saturation, `0..1`.
    pub const fn sat(self) -> f32 {
        match self {
            Self::Okhsv(c) => c.s,
            Self::Hsv(c) => c.s,
        }
    }

    /// Value, `0..1`.
    pub const fn val(self) -> f32 {
        match self {
            Self::Okhsv(c) => c.v,
            Self::Hsv(c) => c.v,
        }
    }

    /// Set the hue. Wraps, so a drag past either end continues round.
    pub fn set_hue(&mut self, h: f32) {
        let h = h.rem_euclid(1.0);
        match self {
            Self::Okhsv(c) => c.h = h,
            Self::Hsv(c) => c.h = h,
        }
    }

    /// Set the saturation, clamped to `0..1`.
    pub fn set_sat(&mut self, s: f32) {
        let s = s.clamp(0.0, 1.0);
        match self {
            Self::Okhsv(c) => c.s = s,
            Self::Hsv(c) => c.s = s,
        }
    }

    /// Set the value, clamped to `0..1`.
    pub fn set_val(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        match self {
            Self::Okhsv(c) => c.v = v,
            Self::Hsv(c) => c.v = v,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::RgbaF32;
    use crate::primitives::color::color_coords::ColorCoords;
    use crate::primitives::color::color_model::ColorModel;

    /// A model switch keeps the colour and moves only the axes.
    #[test]
    fn switching_model_keeps_the_colour() {
        for model in ColorModel::ALL {
            let start = ColorCoords::new(model, RgbaF32::hex(0x4cd3ff), 0.0);
            let other = start.with_model(match model {
                ColorModel::Okhsv => ColorModel::Hsv,
                ColorModel::Hsv => ColorModel::Okhsv,
            });
            assert_ne!(other.model(), start.model());
            let (got, want) = (
                other.to_color().to_srgba_u8(),
                start.to_color().to_srgba_u8(),
            );
            for (g, w) in [(got.r, want.r), (got.g, want.g), (got.b, want.b)] {
                let delta = i16::from(g) - i16::from(w);
                assert!(delta.abs() <= 1, "{model:?}: {want:?} became {got:?}");
            }
        }
    }

    /// Grey has no hue in either model, so a switch and a switch back must
    /// carry the retained one through.
    #[test]
    fn switching_model_keeps_greys_hue() {
        let mut coords = ColorCoords::new(ColorModel::Okhsv, RgbaF32::hex(0x808080), 0.0);
        coords.set_hue(0.42);
        let round_trip = coords
            .with_model(ColorModel::Hsv)
            .with_model(ColorModel::Okhsv);
        assert!(
            (round_trip.hue() - 0.42).abs() < 1e-6,
            "{}",
            round_trip.hue()
        );
    }

    /// Switching to the model already in use is the identity, axes included —
    /// re-deriving would quietly move the hue of a grey.
    #[test]
    fn switching_to_the_same_model_changes_nothing() {
        let mut coords = ColorCoords::new(ColorModel::Okhsv, RgbaF32::BLACK, 0.0);
        coords.set_hue(0.3);
        assert_eq!(coords.with_model(ColorModel::Okhsv), coords);
    }

    #[test]
    fn setters_wrap_the_hue_and_clamp_the_rest() {
        let mut coords = ColorCoords::default();
        coords.set_hue(1.25);
        coords.set_sat(2.0);
        coords.set_val(-1.0);
        assert!((coords.hue() - 0.25).abs() < 1e-6);
        assert_eq!(coords.sat(), 1.0);
        assert_eq!(coords.val(), 0.0);
    }
}
