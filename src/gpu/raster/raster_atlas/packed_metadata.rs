//! A raster's extents and bearing, narrowed to the widths an atlas slot carries.

use glam::{I16Vec2, IVec2, U16Vec2, UVec2};

/// A raster's extents and bearing narrowed to the atlas's widths; what doesn't fit could never have been packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedMetadata {
    pub(super) size: U16Vec2,
    /// Offset from the pen to the raster's top-left; `x` right, `y` **up**.
    pub(super) bearing: I16Vec2,
}

impl PackedMetadata {
    /// `None` when out of range; the caller treats that as too big to cache.
    pub(crate) fn new(size: UVec2, bearing: IVec2) -> Option<Self> {
        Some(Self {
            size: U16Vec2::new(size.x.try_into().ok()?, size.y.try_into().ok()?),
            bearing: I16Vec2::new(bearing.x.try_into().ok()?, bearing.y.try_into().ok()?),
        })
    }

    /// Whether this raster covers no pixels (whitespace); cached so the miss is paid once, but owns no rectangle.
    pub(crate) const fn is_empty(self) -> bool {
        self.size.x == 0 || self.size.y == 0
    }
}
