//! A raster's extents and bearing, narrowed to the widths an atlas slot
//! can afford to carry.

use glam::{I16Vec2, IVec2, U16Vec2, UVec2};

/// A raster's extents and bearing, narrowed to the widths the atlas
/// stores them at. An atlas side tops out far below `u16::MAX`, so
/// anything that does not fit here could never have been packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedMetadata {
    pub(super) size: U16Vec2,
    /// Offset from the pen position to the raster's top-left, in the
    /// rasterizer's sense: `x` right, `y` **up**.
    pub(super) bearing: I16Vec2,
}

impl PackedMetadata {
    /// Narrow a rasterizer's extents and bearing into the atlas's packed
    /// form. `None` when any of them is out of range, which the caller
    /// treats as "too big to cache" rather than an error.
    pub(crate) fn new(size: UVec2, bearing: IVec2) -> Option<Self> {
        Some(Self {
            size: U16Vec2::new(size.x.try_into().ok()?, size.y.try_into().ok()?),
            bearing: I16Vec2::new(bearing.x.try_into().ok()?, bearing.y.try_into().ok()?),
        })
    }

    /// Whether this raster covers no pixels — a whitespace glyph, or one
    /// the rasterizer produced nothing for. Such an entry is cached
    /// (so the miss is paid once) but owns no rectangle.
    pub(crate) fn is_empty(self) -> bool {
        self.size.x == 0 || self.size.y == 0
    }
}
