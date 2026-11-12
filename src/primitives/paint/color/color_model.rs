//! Which set of axes a picker drives.

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::hsv::{self, HsvBlock};
use crate::primitives::paint::color::okhsv::{self, OkhsvBlock, OkhsvSlice};
use crate::primitives::paint::color::srgba_u8::SrgbaU8;

/// The color model a picker's field and hue bar work in, and a [`ColorCoords`](crate::ColorCoords)
/// reads its axes in. In both, every axis is `0..1` and the hue wraps. Serialized so a host can
/// persist the pick.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "lowercase")]
pub enum ColorModel {
    /// Björn Ottosson's Okhsv (<https://bottosson.github.io/posts/colorpicker/>), in linear light;
    /// the default, for its perceptual axes. `s = 1` is the sRGB gamut edge and `v = 1` its
    /// brightest slice, so **every triple in the unit cube is in gamut**, and the hue stays put as
    /// the other axes move.
    ///
    /// # The blue sliver
    ///
    /// A wedge around pure blue is unreachable: Okhsv's gamut edge is the *first* crossing, so
    /// `#0000ff` sits just outside the cube (`s = 1, v = 1` at blue's hue is `#0037ff`). Keep hex,
    /// channels and [`Self::Hsv`] available in a picker.
    #[default]
    Okhsv,
    /// Classic HSV, on sRGB-encoded components as other tools read it, so a number copied out of
    /// one still means what it says.
    Hsv,
}

impl ColorModel {
    /// Both models, in picker order.
    pub const ALL: [Self; 2] = [Self::Okhsv, Self::Hsv];

    /// This model's slice at `hue`, with the per-hue solve done once; see [`HueSlice`].
    pub(crate) fn slice(self, hue: f32) -> HueSlice {
        match self {
            Self::Okhsv => HueSlice::Okhsv(okhsv::slice(hue)),
            Self::Hsv => HueSlice::Hsv(hue),
        }
    }
}

/// One hue of one model, ready to answer a run of samples. Every texel of a field shares the hue,
/// and the Okhsv per-hue gamut solve is costly, so it happens once here. Take one from
/// [`ColorModel::slice`].
#[derive(Clone, Copy, Debug)]
pub(crate) enum HueSlice {
    /// One hue of [`ColorModel::Okhsv`], gamut already solved.
    Okhsv(OkhsvSlice),
    /// One hue of [`ColorModel::Hsv`], which needs no solve.
    Hsv(f32),
}

impl HueSlice {
    /// The opaque color at `s` and `v` on this hue. Both clamp to `0..1`.
    pub(crate) fn color(self, s: f32, v: f32) -> RgbaF32 {
        match self {
            Self::Okhsv(slice) => slice.color(s, v),
            Self::Hsv(hue) => hsv::color(hue, s, v),
        }
    }

    /// This hue at each saturation of `sats`, at most [`HUE_BLOCK`], which clamp: what every
    /// value of those columns shares, solved once.
    pub(crate) fn block(self, sats: impl ExactSizeIterator<Item = f32>) -> HueBlock {
        debug_assert!(sats.len() <= HUE_BLOCK, "{} columns in a block", sats.len());
        match self {
            Self::Okhsv(slice) => HueBlock::Okhsv(slice.block(sats)),
            Self::Hsv(hue) => HueBlock::Hsv(HsvBlock::new(hue, sats)),
        }
    }
}

/// Columns a [`HueBlock`] holds: a cache line of 8-bit texels.
pub(crate) const HUE_BLOCK: usize = 16;

/// Up to [`HUE_BLOCK`] columns of one hue, side by side, answering a row of them at once. Take
/// one from [`HueSlice::block`].
#[derive(Clone, Copy, Debug)]
pub(crate) enum HueBlock {
    Okhsv(OkhsvBlock<HUE_BLOCK>),
    Hsv(HsvBlock<HUE_BLOCK>),
}

impl HueBlock {
    /// `slice.color(s, v).to_srgba8()` for each column's saturation `s`, into `out`, a texel a
    /// column from the first; `v` clamps to `0..1`.
    pub(crate) fn row(&self, v: f32, out: &mut [SrgbaU8]) {
        debug_assert!(
            out.len() <= HUE_BLOCK,
            "{} texels in a block's row",
            out.len()
        );
        match self {
            Self::Okhsv(block) => block.row(v, out),
            Self::Hsv(block) => block.row(v, out),
        }
    }
}
