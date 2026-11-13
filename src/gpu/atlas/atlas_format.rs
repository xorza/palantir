//! [`AtlasFormat`]: the texel format of one page list, and [`PerFormat`], a value for each.

use std::ops::{Index, IndexMut};

/// The texel format a page list holds, one list each: glyph and icon masks and the shadow cutout
/// tables in `R8`, color glyphs and icons in `Rgba8`, backdrop results in `Rgba16F`
/// (`src/gpu/AGENTS.md`, Atlases). The discriminants index a [`PerFormat`].
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

    /// How many formats there are.
    pub(crate) const COUNT: usize = Self::ALL.len();

    /// The bytes of one texel.
    pub(crate) const fn texel_bytes(self) -> u32 {
        match self {
            Self::R8 => 1,
            Self::Rgba8 => 4,
            Self::Rgba16F => 8,
        }
    }
}

/// One `T` for each [`AtlasFormat`], indexed by it: the pool's lists, their textures, a pipeline a
/// format. As `enum_map`'s `EnumMap` keys an array by an enum
/// ([docs.rs/enum-map](https://docs.rs/enum-map)). Refused: that dependency for three formats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PerFormat<T>([T; AtlasFormat::COUNT]);

impl<T> PerFormat<T> {
    /// Each format's value, as `f` makes it.
    pub(crate) fn from_fn(f: impl FnMut(AtlasFormat) -> T) -> Self {
        Self(AtlasFormat::ALL.map(f))
    }

    /// Each format and its value to change, in [`AtlasFormat::ALL`] order.
    pub(crate) fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (AtlasFormat, &mut T)> {
        AtlasFormat::ALL.into_iter().zip(&mut self.0)
    }
}

impl<T> Index<AtlasFormat> for PerFormat<T> {
    type Output = T;

    fn index(&self, format: AtlasFormat) -> &T {
        &self.0[format as usize]
    }
}

impl<T> IndexMut<AtlasFormat> for PerFormat<T> {
    fn index_mut(&mut self, format: AtlasFormat) -> &mut T {
        &mut self.0[format as usize]
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::atlas::atlas_format::{AtlasFormat, PerFormat};

    /// `ALL` lists each format at its discriminant, which indexes a [`PerFormat`]: a value made by
    /// format reads back under it.
    #[test]
    fn every_format_sits_at_its_discriminant() {
        for (at, format) in AtlasFormat::ALL.into_iter().enumerate() {
            assert_eq!(format as usize, at);
        }
        assert_eq!(AtlasFormat::ALL.map(AtlasFormat::texel_bytes), [1, 4, 8]);
        let mut bytes = PerFormat::from_fn(AtlasFormat::texel_bytes);
        assert_eq!(bytes[AtlasFormat::Rgba8], 4);
        for (format, value) in bytes.iter_mut() {
            *value += format as u32;
        }
        assert_eq!(AtlasFormat::ALL.map(|format| bytes[format]), [1, 5, 10]);
    }
}
