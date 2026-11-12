//! [`AtlasFormat`]: the texel format of one page list.

/// The texel format a page list holds, one list each: glyph and icon masks and the shadow cutout
/// tables in `R8`, color glyphs and icons in `Rgba8`, backdrop results in `Rgba16F`
/// (`.notes/proposals/atlas-pool.md`, Packed pages). The discriminants index the pool's lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum AtlasFormat {
    R8 = 0,
    Rgba8 = 1,
    Rgba16F = 2,
}

impl AtlasFormat {
    /// Every format, in discriminant order.
    pub(crate) const ALL: [Self; 3] = [Self::R8, Self::Rgba8, Self::Rgba16F];

    /// The bytes of one texel.
    pub(crate) const fn texel_bytes(self) -> u32 {
        match self {
            Self::R8 => 1,
            Self::Rgba8 => 4,
            Self::Rgba16F => 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::atlas::atlas_format::AtlasFormat;

    /// `ALL` lists each format at its discriminant, which indexes the pool's lists.
    #[test]
    fn every_format_sits_at_its_discriminant() {
        for (at, format) in AtlasFormat::ALL.into_iter().enumerate() {
            assert_eq!(format as usize, at);
        }
        assert_eq!(AtlasFormat::ALL.map(AtlasFormat::texel_bytes), [1, 4, 8]);
    }
}
