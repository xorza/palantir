//! sRGB-encoded bytes: what a hex code, an image texel, or a number shown
//! to a person means.

/// A 4-byte **sRGB-encoded** colour with a straight 8-bit alpha.
///
/// The one form in the crate that is not linear, and its own type so it
/// cannot be handed where linear bytes are wanted: a [`RgbaU8`] fed into a
/// gradient LUT that was really this would paint far too bright, and a
/// texel written from a [`RgbaU8`] would paint far too dark. Both mistakes
/// were one method call apart while they shared a type.
///
/// Built from a hex literal, or by [`RgbaF32::to_srgba_u8`]. Read back
/// through [`RgbaF32::from_srgba`], which decodes.
///
/// [`RgbaU8`]: crate::RgbaU8
/// [`RgbaF32`]: crate::RgbaF32
/// [`RgbaF32::to_srgba_u8`]: crate::RgbaF32::to_srgba_u8
/// [`RgbaF32::from_srgba`]: crate::RgbaF32::from_srgba
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
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
}
