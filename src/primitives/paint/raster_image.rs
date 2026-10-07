//! [`RasterImage`]: pixels on their way into an atlas. In primitives because it is the seam between `text` and `icons` (producers) and `renderer` (consumer); any one of them as home would make the others depend upward.

use crate::primitives::paint::content_type::ContentType;
use glam::{IVec2, UVec2};

/// One rasterized image, borrowed from the rasterizer that produced it. Swash (glyphs) and resvg (icons) render into kept scratch, so a zoom gesture re-rasterizing a screenful allocates nothing after frame one; the atlas copies the bytes before the next raster overwrites them.
#[derive(Clone, Copy, Debug)]
pub struct RasterImage<'a> {
    /// Whether [`Self::data`] is one byte per pixel or four.
    pub content: ContentType,
    /// Raster dimensions in physical pixels.
    pub size: UVec2,
    /// Offset from the pen to the raster's top-left: `x` right, `y` **up**. Zero for an icon.
    pub bearing: IVec2,
    /// Tightly packed rows, `size.x * size.y` pixels; one byte each for
    /// [`ContentType::Mask`], four (RGBA) for [`ContentType::Color`].
    pub data: &'a [u8],
}
