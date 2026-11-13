//! [`ContentType`]: single-channel coverage, or full color. Beside [`RasterImage`](crate::primitives::paint::raster_image::RasterImage) in primitives, since both rasterizers and the atlas name it.

/// What a raster's bytes hold, and so which of the atlas pool's lists it lives in. The
/// discriminants are load-bearing: a raster quad packs its content as its kind bit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ContentType {
    /// One coverage byte per pixel, multiplied by the shape's tint so one raster serves every theme
    /// color.
    Mask = 0,
    /// Straight sRGB RGBA, four bytes per pixel; premultiplied in linear at output.
    Color = 1,
}
