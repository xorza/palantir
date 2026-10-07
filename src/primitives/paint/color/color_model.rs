//! Which set of axes a picker drives.

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::hsv::Hsv;
use crate::primitives::paint::color::okhsv::{Okhsv, OkhsvSlice};

/// The colour model a picker's field and hue bar work in. [`Okhsv`] is the default for its perceptual axes; [`Hsv`] matches numbers from other tools. Serialized so a host can persist the pick.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ColorModel {
    /// Perceptual HSV over Oklab (default).
    #[default]
    Okhsv,
    /// Classic sRGB-space HSV.
    Hsv,
}

impl ColorModel {
    /// Both models, in picker order.
    pub const ALL: [Self; 2] = [Self::Okhsv, Self::Hsv];

    /// What the model switch calls this one.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Okhsv => "Okhsv",
            Self::Hsv => "HSV",
        }
    }

    /// This model's slice at `hue`, with the per-hue solve done once; see [`HueSlice`].
    pub fn slice(self, hue: f32) -> HueSlice {
        match self {
            Self::Okhsv => HueSlice(HueSliceKind::Okhsv(Okhsv::slice(hue))),
            Self::Hsv => HueSlice(HueSliceKind::Hsv(hue)),
        }
    }
}

/// One hue of one model, ready to answer a run of samples. Every texel of a field shares the hue, and the Okhsv per-hue gamut solve is costly, so it happens once here. Take one from [`ColorModel::slice`].
#[derive(Clone, Copy, Debug)]
pub struct HueSlice(HueSliceKind);

#[derive(Clone, Copy, Debug)]
enum HueSliceKind {
    /// One hue of [`ColorModel::Okhsv`], gamut already solved.
    Okhsv(OkhsvSlice),
    /// One hue of [`ColorModel::Hsv`], which needs no solve.
    Hsv(f32),
}

impl HueSlice {
    /// The opaque colour at `s` and `v` on this hue. Both clamp to `0..1`.
    pub fn color(self, s: f32, v: f32) -> RgbaF32 {
        match self.0 {
            HueSliceKind::Okhsv(slice) => slice.color(s, v),
            HueSliceKind::Hsv(hue) => Hsv::new(hue, s, v).to_color(),
        }
    }
}
