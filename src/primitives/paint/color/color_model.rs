//! Which set of axes a picker drives.

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::hsv::{Hsv, HsvBlock};
use crate::primitives::paint::color::okhsv::{Okhsv, OkhsvBlock, OkhsvSlice};
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

    /// This hue at each saturation of `sats`, at most [`HUE_BLOCK`], which clamp: what every
    /// value of those columns shares, solved once.
    pub(crate) fn block(self, sats: impl ExactSizeIterator<Item = f32>) -> HueBlock {
        debug_assert!(sats.len() <= HUE_BLOCK, "{} columns in a block", sats.len());
        HueBlock(match self.0 {
            HueSliceKind::Okhsv(slice) => HueBlockKind::Okhsv(slice.block(sats)),
            HueSliceKind::Hsv(hue) => HueBlockKind::Hsv(HsvBlock::new(hue, sats)),
        })
    }
}

/// Columns a [`HueBlock`] holds: a cache line of 8-bit texels.
pub(crate) const HUE_BLOCK: usize = 16;

/// Up to [`HUE_BLOCK`] columns of one hue, side by side, answering a row of them at once. Take
/// one from [`HueSlice::block`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct HueBlock(HueBlockKind);

#[derive(Clone, Copy, Debug)]
enum HueBlockKind {
    Okhsv(OkhsvBlock<HUE_BLOCK>),
    Hsv(HsvBlock<HUE_BLOCK>),
}

impl HueBlock {
    /// `slice.color(s, v).to_srgba()` for each column's saturation `s`, into `out`, a texel a
    /// column from the first; `v` clamps to `0..1`.
    pub(crate) fn row(&self, v: f32, out: &mut [SrgbaU8]) {
        debug_assert!(
            out.len() <= HUE_BLOCK,
            "{} texels in a block's row",
            out.len()
        );
        match &self.0 {
            HueBlockKind::Okhsv(block) => block.row(v, out),
            HueBlockKind::Hsv(block) => block.row(v, out),
        }
    }
}
