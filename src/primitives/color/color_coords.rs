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

    /// Set the hue, clamped to `0..=1`. Both ends name red, and both are
    /// kept: a hue bar dragged to its right edge reads 1 and draws its
    /// marker there, where a wrap to 0 jumped it to the left. A caller
    /// stepping round the circle wraps its own arithmetic.
    pub fn set_hue(&mut self, h: f32) {
        let h = h.clamp(0.0, 1.0);
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
    use crate::primitives::approx::test_support::assert_close;
    use crate::primitives::color::RgbaF32;
    use crate::primitives::color::color_coords::ColorCoords;
    use crate::primitives::color::color_model::ColorModel;
    use crate::primitives::color::hsv::Hsv;
    use crate::primitives::color::okhsv::Okhsv;

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
            assert_eq!(got, want, "{model:?}");
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
        assert_eq!(round_trip.hue(), 0.42, "{}", round_trip.hue());
    }

    /// Switching to the model already in use is the identity, axes included —
    /// re-deriving would quietly move the hue of a grey.
    #[test]
    fn switching_to_the_same_model_changes_nothing() {
        let mut coords = ColorCoords::new(ColorModel::Okhsv, RgbaF32::BLACK, 0.0);
        coords.set_hue(0.3);
        assert_eq!(coords.with_model(ColorModel::Okhsv), coords);
    }

    /// Every setter clamps, the hue included: 1.0 stays 1.0 — red, as 0.0
    /// is — rather than wrapping to 0.0, and 1.25 clamps to it.
    #[test]
    fn setters_clamp() {
        let mut coords = ColorCoords::default();
        coords.set_hue(1.0);
        assert_eq!(coords.hue(), 1.0);
        coords.set_hue(1.25);
        coords.set_sat(2.0);
        coords.set_val(-1.0);
        assert_eq!(coords.hue(), 1.0);
        assert_eq!(coords.sat(), 1.0);
        assert_eq!(coords.val(), 0.0);
        assert_eq!(
            coords.to_color().to_srgba_u8(),
            ColorCoords::default().to_color().to_srgba_u8(),
            "hue 1 is the colour hue 0 is",
        );
    }

    /// `model`'s coordinates at the raw axes `(h, s, v)` — through the
    /// model's own constructor, which wraps and clamps, not the setters.
    fn axes(model: ColorModel, h: f32, s: f32, v: f32) -> ColorCoords {
        match model {
            ColorModel::Okhsv => ColorCoords::Okhsv(Okhsv::new(h, s, v)),
            ColorModel::Hsv => ColorCoords::Hsv(Hsv::new(h, s, v)),
        }
    }

    /// Distance between two hues the short way round the circle.
    fn hue_gap(a: f32, b: f32) -> f32 {
        let raw = (a - b).abs();
        raw.min(1.0 - raw)
    }

    /// What both models promise alike. Every triple in the unit cube is
    /// inside sRGB, so nothing clamps away and the round trip holds — 9 × 8
    /// × 8 samples, grey excluded, since it has no hue to recover. Grey
    /// keeps the fallback hue instead. And the axes past their ends take
    /// what a drag drives them to: the hue wraps, the other two clamp.
    #[test]
    fn both_models_round_trip_keep_greys_hue_and_wrap_or_clamp() {
        for model in ColorModel::ALL {
            let mut worst = 0.0_f32;
            for hi in 0..9 {
                for si in 1..9 {
                    for vi in 1..9 {
                        let (h, s, v) = (hi as f32 / 9.0, si as f32 / 8.0, vi as f32 / 8.0);
                        let back = ColorCoords::new(model, axes(model, h, s, v).to_color(), h);
                        worst = worst
                            .max(hue_gap(back.hue(), h))
                            .max((back.sat() - s).abs())
                            .max((back.val() - v).abs());
                    }
                }
            }
            assert_close(
                worst,
                0.0,
                1e-3,
                "the worst axis drift over the cube, through f32 conversions",
            );

            for level in [0.0, 0.25, 0.5, 1.0] {
                let grey = ColorCoords::new(model, RgbaF32::srgb(level, level, level), 0.618);
                assert_eq!(grey.hue(), 0.618, "{model:?}: grey at {level}");
                assert_close(
                    grey.sat(),
                    0.0,
                    1e-3,
                    "grey's saturation, to the model's f32 rounding",
                );
            }

            let byte = |c: ColorCoords| c.to_color().to_srgba_u8();
            assert_eq!(
                byte(axes(model, 1.25, 2.0, 2.0)),
                byte(axes(model, 0.25, 1.0, 1.0)),
                "{model:?}: past the top",
            );
            assert_eq!(
                byte(axes(model, -0.75, -1.0, 0.5)),
                byte(axes(model, 0.25, 0.0, 0.5)),
                "{model:?}: past the bottom",
            );
        }
    }
}
