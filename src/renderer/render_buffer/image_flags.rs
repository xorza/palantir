//! The sampling flags an image instance carries to the shader.

use bytemuck::{Pod, Zeroable};

/// How the image shader samples one instance. `repr(transparent)` over the
/// `u32` vertex attribute the shader reads, which takes every bit as a
/// substituted constant.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct ImageFlags(u32);

impl ImageFlags {
    pub(crate) const NONE: Self = Self(0);
    /// Wrap UVs with `fract` in the shader (`ImageFit::Tile`).
    pub(crate) const TILED: Self = Self(1 << 0);
    /// Nearest-neighbour minification.
    pub(crate) const MIN_NEAREST: Self = Self(1 << 1);
    /// Nearest-neighbour magnification.
    pub(crate) const MAG_NEAREST: Self = Self(1 << 2);
    /// Where the image minifies, spread a grid of taps over the fragment's
    /// source footprint and average them
    /// ([`ImageDownsample::Mean`](crate::ImageDownsample::Mean)).
    pub(crate) const TAPS_MEAN: Self = Self(1 << 3);
    /// As [`Self::TAPS_MEAN`], but the brightest tap wins instead of the
    /// average ([`ImageDownsample::Peak`](crate::ImageDownsample::Peak)).
    /// Mutually exclusive with it — the encoder sets at most one.
    pub(crate) const TAPS_PEAK: Self = Self(1 << 4);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The bits as the shader masks them.
    pub(crate) const fn bits(self) -> u32 {
        self.0
    }
}
