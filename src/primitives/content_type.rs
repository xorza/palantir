//! [`ContentType`] — single-channel coverage, or full colour.
//!
//! Beside [`RasterImage`](crate::primitives::raster_image::RasterImage)
//! and at the primitives layer for the same reason: both rasterizers name
//! it and so does the atlas, and none of the three sits above the others.

/// What a raster's bytes hold, and so which of an atlas's two sides it
/// lives on.
///
/// One answer for every rasterizer in the crate. A glyph is a swash
/// bitmap and an icon a rendered SVG, but each is one of these two things
/// and each says so in the same word — see
/// [`RasterImage::content`](crate::RasterImage).
///
/// The discriminants are load-bearing: `RasterAtlas` indexes its
/// `[Side; 2]` with `content as usize`, and a `PendingCopy` stores the
/// side it targets as a `u8`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ContentType {
    /// One coverage byte per pixel. The draw multiplies it by the shape's
    /// full tint, so one baked raster serves every theme colour.
    Mask = 0,
    /// Straight (non-premultiplied) sRGB RGBA, four bytes per pixel — what
    /// the colour atlas side stores and what the raster shader
    /// premultiplies in linear at output.
    Color = 1,
}

impl ContentType {
    pub(crate) fn format(self) -> wgpu::TextureFormat {
        match self {
            Self::Mask => wgpu::TextureFormat::R8Unorm,
            Self::Color => wgpu::TextureFormat::Rgba8UnormSrgb,
        }
    }

    pub(crate) fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Mask => 1,
            Self::Color => 4,
        }
    }

    pub(crate) fn side_name(self) -> &'static str {
        match self {
            Self::Mask => "mask",
            Self::Color => "color",
        }
    }
}
