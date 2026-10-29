//! Which set of axes a picker drives.

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::hsv::{Hsv, HsvColumn};
use crate::primitives::paint::color::okhsv::{Okhsv, OkhsvColumn, OkhsvSlice};
use crate::primitives::paint::color::srgba_u8::SrgbaU8;

/// The color model a picker's field and hue bar work in. [`Okhsv`] is the default for its perceptual axes; [`Hsv`] matches numbers from other tools. Serialized so a host can persist the pick.
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
    /// The opaque color at `s` and `v` on this hue. Both clamp to `0..1`.
    pub fn color(self, s: f32, v: f32) -> RgbaF32 {
        match self.0 {
            HueSliceKind::Okhsv(slice) => slice.color(s, v),
            HueSliceKind::Hsv(hue) => Hsv::new(hue, s, v).to_color(),
        }
    }

    /// This hue at saturation `s`, which clamps, ready to answer a run of values.
    pub(crate) fn column(self, s: f32) -> HueColumn {
        HueColumn(match self.0 {
            HueSliceKind::Okhsv(slice) => HueColumnKind::Okhsv(slice.column(s)),
            HueSliceKind::Hsv(hue) => HueColumnKind::Hsv(HsvColumn::new(hue, s)),
        })
    }
}

/// One hue and saturation of one model, with what every value shares solved once. Take one
/// from [`HueSlice::column`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct HueColumn(HueColumnKind);

#[derive(Clone, Copy, Debug)]
enum HueColumnKind {
    Okhsv(OkhsvColumn),
    Hsv(HsvColumn),
}

impl HueColumn {
    /// `slice.color(s, v).to_srgba()` for the slice and saturation this column holds; `v`
    /// clamps to `0..1`.
    #[inline]
    pub(crate) fn to_srgba(self, v: f32) -> SrgbaU8 {
        match self.0 {
            HueColumnKind::Okhsv(column) => column.color(v).to_srgba(),
            HueColumnKind::Hsv(column) => column.to_srgba(v),
        }
    }
}
