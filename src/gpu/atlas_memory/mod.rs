//! What each GPU atlas holds: the bytes of its textures against the bytes its live items fill,
//! which the pool's budgets read over the bench scenes
//! (`cargo atlas-memory`).

pub(crate) mod bench;

use std::fmt::{self, Display};

/// The memory of one atlas, or of one set of textures beside them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasUse {
    pub(crate) name: &'static str,
    /// The textures it holds.
    pub(crate) textures: u32,
    /// Their bytes, every mip level counted.
    pub(crate) bytes: u64,
    /// The bytes its live items fill, `None` for scratch that holds no items.
    pub(crate) live_bytes: Option<u64>,
}

impl AtlasUse {
    /// The memory `textures` hold, every one a texture of `name` with no live items.
    pub(crate) fn scratch<'a>(
        name: &'static str,
        textures: impl IntoIterator<Item = &'a wgpu::Texture>,
    ) -> Self {
        let (count, bytes) = textures
            .into_iter()
            .fold((0, 0), |(count, bytes), texture| {
                (count + 1, bytes + texture_bytes(texture))
            });
        Self {
            name,
            textures: count,
            bytes,
            live_bytes: None,
        }
    }
}

/// Every atlas of one device, and the CPU mirror of its gradient rows, in a fixed order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasMemory {
    pub(crate) uses: [AtlasUse; Self::LEN],
}

impl AtlasMemory {
    /// The rows a report holds: the pool's mask pages, which glyphs, icons and shadow cutouts
    /// share, its color pages, which glyphs and icons share, its half-float pages and scratch,
    /// which backdrop results take, the blur's textures, gradient rows and their CPU staging.
    pub(crate) const LEN: usize = 6;
}

/// The bytes of `texture`: every mip level of every layer, a block a texel.
///
/// # Panics
///
/// Panics for a depth or multi-planar format, which no atlas takes.
pub(crate) fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    let texel = u64::from(
        texture
            .format()
            .block_copy_size(None)
            .expect("an atlas holds a color format"),
    );
    let layers = u64::from(texture.depth_or_array_layers());
    (0..texture.mip_level_count())
        .map(|mip| {
            let w = u64::from((texture.width() >> mip).max(1));
            let h = u64::from((texture.height() >> mip).max(1));
            w * h * texel * layers
        })
        .sum()
}

/// A byte count as a person reads it: bytes below a KiB, else KiB or MiB to one decimal, so a
/// nonzero count never reads as zero.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bytes(pub(crate) u64);

impl Display for Bytes {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a byte count prints to one decimal of a KiB or a MiB"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const KIB: u64 = 1 << 10;
        const MIB: u64 = 1 << 20;
        let text = match self.0 {
            n if n < KIB => format!("{n} B"),
            n if n < MIB => format!("{:.1} KiB", n as f64 / KIB as f64),
            n => format!("{:.1} MiB", n as f64 / MIB as f64),
        };
        f.pad(&text)
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::atlas_memory::{Bytes, texture_bytes};
    use crate::gpu::test_gpu::headless_test_gpu;

    /// A texture's bytes sum its mip levels: a 64² `Rgba16Float` with three levels holds
    /// `(64² + 32² + 16²) · 8 = (4096 + 1024 + 256) · 8 = 43008` bytes, an `R8Unorm` of 3 × 5 one
    /// mip `15`.
    #[test]
    fn texture_bytes_sum_every_level() {
        let gpu = headless_test_gpu();
        let texture = |format, size: wgpu::Extent3d, mip_level_count| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let extent = |width, height| wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        assert_eq!(
            texture_bytes(&texture(
                wgpu::TextureFormat::Rgba16Float,
                extent(64, 64),
                3
            )),
            43008
        );
        assert_eq!(
            texture_bytes(&texture(wgpu::TextureFormat::R8Unorm, extent(3, 5), 1)),
            15
        );
    }

    /// A count reads in bytes below a KiB, then in KiB and MiB to a decimal: `1023 B`, `1.0 KiB`,
    /// `1.5 KiB` for 1536, `1.0 MiB` for 2²⁰ and `24.0 MiB` for 24 · 2²⁰; one byte is `1 B`, never
    /// `0.0 KiB`.
    #[test]
    fn bytes_read_in_kib_and_mib() {
        let read = |n| Bytes(n).to_string();
        assert_eq!(read(0), "0 B");
        assert_eq!(read(1), "1 B");
        assert_eq!(read(1023), "1023 B");
        assert_eq!(read(1024), "1.0 KiB");
        assert_eq!(read(1536), "1.5 KiB");
        assert_eq!(read(1 << 20), "1.0 MiB");
        assert_eq!(read(24 << 20), "24.0 MiB");
    }
}
