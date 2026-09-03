//! [`RasterImage`] — pixels on their way into an atlas.
//!
//! At the primitives layer because it is the seam between three others:
//! `text` and `icons` each produce one, and `renderer` consumes both. A
//! home under any of the three would make the other two depend upward on
//! it.

use crate::primitives::content_type::ContentType;
use glam::{IVec2, UVec2};

/// One rasterized image, borrowed from the rasterizer that produced it.
///
/// A glyph comes from swash and an icon from resvg, and neither hands
/// back an owned buffer: each renders into scratch it keeps, so a zoom
/// gesture that re-rasterizes a screenful allocates nothing after its
/// first frame. The borrow is what enforces that — an owned `Vec` here
/// would be one allocation per raster, and the atlas copies the bytes
/// out before the next raster overwrites them.
#[derive(Clone, Copy, Debug)]
pub struct RasterImage<'a> {
    pub content: ContentType,
    pub size: UVec2,
    /// Offset from the pen position to the raster's top-left, in the
    /// rasterizer's sense: `x` right, `y` **up**. Zero for an icon,
    /// whose raster *is* its box.
    pub bearing: IVec2,
    /// Tightly packed rows, `size.x * size.y` pixels; one byte each for
    /// [`ContentType::Mask`], four (RGBA) for [`ContentType::Color`].
    pub data: &'a [u8],
}
