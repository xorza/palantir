//! sRGB-encoded bytes: what a hex code, an image texel, or a number shown
//! to a person means.

use crate::primitives::color::{RgbaF16, RgbaF32, RgbaU8};

/// A 4-byte **sRGB-encoded** colour with a straight 8-bit alpha.
///
/// The one form in the crate that is not linear, and its own type so it
/// cannot be handed where linear bytes are wanted: a [`RgbaU8`] fed into a
/// gradient LUT that was really this would paint far too bright, and a
/// texel written from a [`RgbaU8`] would paint far too dark. Both mistakes
/// were one method call apart while they shared a type.
///
/// Built from a hex literal, by [`RgbaF32::to_srgba_u8`], or through
/// `From` from any other colour type. Read back through
/// [`RgbaF32::from_srgba`], which decodes.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrgbaU8 {
    /// Red, sRGB-encoded, 0..255.
    pub r: u8,
    /// Green, sRGB-encoded, 0..255.
    pub g: u8,
    /// Blue, sRGB-encoded, 0..255.
    pub b: u8,
    /// Alpha, 0..255, straight. Alpha is never gamma-encoded.
    pub a: u8,
}

impl SrgbaU8 {
    /// Bytes stored as given.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Opaque, from three encoded bytes.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xff }
    }

    /// Packed 24-bit `0xRRGGBB` literal, opaque — CSS hex notation, so
    /// `#3366CC` is `SrgbaU8::hex(0x3366CC)`.
    pub const fn hex(rgb: u32) -> Self {
        Self::hexa((rgb << 8) | 0xff)
    }

    /// Packed 32-bit `0xRRGGBBAA` literal, alpha last as CSS orders it.
    ///
    /// `to_be_bytes` *is* the CSS packing — R in the most significant byte —
    /// so the split is the standard library's rather than four hand-written
    /// shifts.
    pub const fn hexa(rgba: u32) -> Self {
        let [r, g, b, a] = rgba.to_be_bytes();
        Self { r, g, b, a }
    }

    /// The four bytes as one `0xRRGGBBAA` word, the inverse of
    /// [`Self::hexa`]: one hasher write instead of four. See
    /// `GradientStops`'s `Hash` for why the byte order matters there.
    #[inline]
    pub(crate) const fn to_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }
}

impl From<RgbaF32> for SrgbaU8 {
    /// The exact encode, [`RgbaF32::to_srgba_u8`].
    #[inline]
    fn from(c: RgbaF32) -> Self {
        c.to_srgba_u8()
    }
}

impl From<RgbaF16> for SrgbaU8 {
    /// Unpack, then the exact encode.
    #[inline]
    fn from(c: RgbaF16) -> Self {
        c.unpack().to_srgba_u8()
    }
}

impl From<RgbaU8> for SrgbaU8 {
    /// Un-quantize, then the exact encode: the sRGB byte nearest each
    /// linear byte. Near white one sRGB step spans several linear steps,
    /// so neighbouring linear bytes there can meet on one sRGB byte.
    #[inline]
    fn from(c: RgbaU8) -> Self {
        c.to_linear().to_srgba_u8()
    }
}
