//! [`ContentType`]: single-channel coverage, or full colour. Beside [`RasterImage`](crate::primitives::paint::raster_image::RasterImage) in primitives, since both rasterizers and the atlas name it.

/// What a raster's bytes hold, and so which of an atlas's two sides it lives on; one answer for every rasterizer (see [`RasterImage::content`](crate::widget::RasterImage)). The discriminants are load-bearing: `RasterAtlas` indexes its `[Side; 2]` with `content as usize`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ContentType {
    /// One coverage byte per pixel, multiplied by the shape's tint so one baked raster serves every theme colour.
    Mask = 0,
    /// Straight sRGB RGBA, four bytes per pixel; stored on the colour side and premultiplied in linear at output.
    Color = 1,
}

impl ContentType {
    pub(crate) const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Mask => 1,
            Self::Color => 4,
        }
    }

    pub(crate) const fn side_name(self) -> &'static str {
        match self {
            Self::Mask => "mask",
            Self::Color => "color",
        }
    }
}
