//! The sampling flags an image instance carries to the shader.

use bytemuck::{Pod, Zeroable};

/// How the image shader samples one instance; `repr(transparent)` over the shader's `u32` vertex attribute.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct ImageFlags(u32);

impl ImageFlags {
    pub(crate) const NONE: Self = Self(0);
    /// Wrap UVs with `fract` in the shader (`ImageFit::Tile`).
    pub(crate) const TILED: Self = Self(1 << 0);
    pub(crate) const MIN_NEAREST: Self = Self(1 << 1);
    pub(crate) const MAG_NEAREST: Self = Self(1 << 2);
    /// Average a grid of taps over the source footprint where the image minifies ([`ImageDownsample::Mean`](crate::ImageDownsample::Mean)).
    pub(crate) const TAPS_MEAN: Self = Self(1 << 3);
    /// As [`Self::TAPS_MEAN`] but the brightest tap wins ([`ImageDownsample::Peak`](crate::ImageDownsample::Peak)); at most one is set.
    pub(crate) const TAPS_PEAK: Self = Self(1 << 4);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn bits(self) -> u32 {
        self.0
    }
}
