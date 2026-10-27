//! sRGB-encoded bytes: what a hex code, an image texel, or a number shown
//! to a person means.

/// A 4-byte **sRGB-encoded** color with straight 8-bit alpha.
///
/// The one non-linear color form, a distinct type so encoded bytes are never
/// read as linear light. Decode with [`RgbaF32::from_srgba`] or `From`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrgbaU8 {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha, 0..255, straight. Alpha is never gamma-encoded.
    pub a: u8,
}

impl SrgbaU8 {
    /// From channels.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Opaque color from channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xff }
    }

    /// Packed `0xRRGGBB` literal, opaque (CSS hex): `#3366CC` is `hex(0x3366CC)`.
    pub const fn hex(rgb: u32) -> Self {
        Self::hexa((rgb << 8) | 0xff)
    }

    /// Packed `0xRRGGBBAA` literal, alpha last as CSS orders it.
    pub const fn hexa(rgba: u32) -> Self {
        let [r, g, b, a] = rgba.to_be_bytes();
        Self { r, g, b, a }
    }

    /// The four bytes as one `0xRRGGBBAA` word, inverse of [`Self::hexa`]; one hasher write. See `GradientStops`'s `Hash`.
    #[inline]
    pub(crate) const fn to_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }
}
