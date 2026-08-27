//! Which of an atlas's two sides a raster lives on: single-channel
//! coverage, or full colour.

/// Which of an atlas's two sides content lives on. `Mask` is one coverage
/// byte per texel and takes the draw's colour; `Color` is straight sRGB RGBA
/// and supplies its own.
///
/// The discriminants are load-bearing: `RasterAtlas` indexes its
/// `[Side; 2]` with `content as usize`, and a `PendingCopy` stores the
/// side it targets as a `u8`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum ContentType {
    Mask = 0,
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

    pub(super) fn side_name(self) -> &'static str {
        match self {
            Self::Mask => "mask",
            Self::Color => "color",
        }
    }
}
