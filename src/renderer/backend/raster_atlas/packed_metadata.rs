//! A raster's extents and bearing, narrowed to the widths an atlas slot
//! can afford to carry.

/// A raster's extents and bearing, narrowed to the widths the atlas
/// stores them at. An atlas side tops out far below `u16::MAX`, so
/// anything that does not fit here could never have been packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedMetadata {
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) left: i16,
    pub(super) top: i16,
}

impl PackedMetadata {
    pub(crate) const EMPTY: Self = Self {
        width: 0,
        height: 0,
        left: 0,
        top: 0,
    };

    /// Narrow a rasterizer's extents and bearing into the atlas's packed
    /// form. `None` when any of them is out of range, which the caller
    /// treats as "too big to cache" rather than an error.
    pub(crate) fn new(width: u32, height: u32, left: i32, top: i32) -> Option<Self> {
        Some(Self {
            width: width.try_into().ok()?,
            height: height.try_into().ok()?,
            left: left.try_into().ok()?,
            top: top.try_into().ok()?,
        })
    }

    /// Whether this raster covers no pixels — a whitespace glyph, or one
    /// the rasterizer produced nothing for. Such an entry is cached
    /// (so the miss is paid once) but owns no rectangle.
    pub(crate) fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}
