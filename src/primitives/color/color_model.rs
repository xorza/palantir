//! Which set of axes a picker drives.

use crate::primitives::color::RgbaF32;
use crate::primitives::color::hsv::Hsv;
use crate::primitives::color::okhsv::{Okhsv, OkhsvSlice};

/// The colour model a picker's field and hue bar work in.
///
/// Two, not more. [`Okhsv`](crate::Okhsv) is the default because its axes are
/// perceptual: the hue holds still while the other two move, and one value
/// reads as one brightness around the whole circle.
/// [`Hsv`](crate::Hsv) is kept because a number matched against another tool
/// has to land where that tool says.
///
/// Serialized so a host can persist which one the user last picked in.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ColorModel {
    #[default]
    Okhsv,
    Hsv,
}

impl ColorModel {
    /// Both models, in the order a picker offers them.
    pub const ALL: [Self; 2] = [Self::Okhsv, Self::Hsv];

    /// What the model switch calls this one.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Okhsv => "Okhsv",
            Self::Hsv => "HSV",
        }
    }

    /// This model's slice at `hue`, with whatever the model solves per hue
    /// solved once. What to build a texture of one hue from — see
    /// [`HueSlice`].
    pub fn slice(self, hue: f32) -> HueSlice {
        match self {
            Self::Okhsv => HueSlice::Okhsv(Okhsv::slice(hue)),
            Self::Hsv => HueSlice::Hsv(hue),
        }
    }
}

/// One hue of one model, ready to answer a run of samples.
///
/// What a colour field's texture is filled from. Every texel of a field
/// shares the hue, and for [`ColorModel::Okhsv`] the per-hue gamut solve is
/// the expensive half of a conversion — so it happens once here rather than
/// four thousand times in the loop. Take one from [`ColorModel::slice`].
#[derive(Clone, Copy, Debug)]
pub enum HueSlice {
    Okhsv(OkhsvSlice),
    Hsv(f32),
}

impl HueSlice {
    /// The opaque colour at `s` and `v` on this hue. Both clamp to `0..1`.
    pub fn color(self, s: f32, v: f32) -> RgbaF32 {
        match self {
            Self::Okhsv(slice) => slice.color(s, v),
            Self::Hsv(hue) => Hsv::new(hue, s, v).to_color(),
        }
    }
}
