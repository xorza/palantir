//! A square texture of packed rectangles: its shelf packer, its edge grown by doubling up to a
//! ceiling, and the copy of its texels into the grown texture.

use crate::gpu::resource::shelf_packer::ShelfPacker;
use glam::U16Vec2;

/// What every texture of one atlas shares through its doublings.
#[derive(Debug)]
pub(crate) struct AtlasSpec {
    /// GPU debug name, built once so a grow reuses it.
    pub(crate) label: String,
    pub(crate) format: wgpu::TextureFormat,
    /// How the atlas's owner uses its texture, beside the copies every atlas takes.
    pub(crate) usage: wgpu::TextureUsages,
    /// The largest edge the atlas grows to.
    pub(crate) ceiling: u32,
}

impl AtlasSpec {
    fn create(&self, device: &wgpu::Device, edge: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&self.label),
            size: wgpu::Extent3d {
                width: edge,
                height: edge,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: self.usage | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }
}

/// A square texture whose rectangles a [`ShelfPacker`] places.
///
/// A grow doubles the packer's edge at once and the texture at [`Self::commit`], so a frame that
/// grows several times makes one texture at the final edge and copies its texels once, where a
/// texture a doubling would make each intermediate one and never sample it.
#[derive(Debug)]
pub(crate) struct PackedAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The edge the packer places rectangles on; the texture's until a grow, which
    /// [`Self::commit`] makes the texture follow.
    edge: u32,
    packer: ShelfPacker,
    spec: AtlasSpec,
}

impl PackedAtlas {
    /// An empty atlas of `spec` at an edge of `initial` texels, held to the ceiling.
    pub(crate) fn new(device: &wgpu::Device, spec: AtlasSpec, initial: u32) -> Self {
        let edge = initial.min(spec.ceiling);
        let texture = spec.create(device, edge);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            edge,
            packer: ShelfPacker::new(edge_u16(edge)),
            spec,
        }
    }

    /// The edge rectangles are placed on, in texels.
    pub(crate) const fn edge(&self) -> u32 {
        self.edge
    }

    /// Whether a rectangle of `size` fits the edge as it stands.
    pub(crate) const fn fits_now(&self, size: U16Vec2) -> bool {
        fits_edge(size, self.edge)
    }

    /// Whether a rectangle of `size` could ever be placed: freeing rectangles cannot widen the
    /// texture past its ceiling.
    pub(crate) const fn fits_ceiling(&self, size: U16Vec2) -> bool {
        fits_edge(size, self.spec.ceiling)
    }

    pub(crate) const fn packer(&self) -> &ShelfPacker {
        &self.packer
    }

    /// The top-left of a free rectangle of `size`, now taken; `None` when the edge as it stands
    /// has no room.
    pub(crate) fn allocate(&mut self, size: U16Vec2) -> Option<U16Vec2> {
        self.packer.allocate(size)
    }

    /// Return the rectangle of `width` at `origin`.
    pub(crate) fn deallocate(&mut self, origin: U16Vec2, width: u16) {
        self.packer.deallocate(origin, width);
    }

    /// Double the edge, held to the ceiling, keeping every placement; `false` at the ceiling.
    pub(crate) fn grow(&mut self) -> bool {
        if self.edge >= self.spec.ceiling {
            return false;
        }
        self.edge = (self.edge * 2).min(self.spec.ceiling);
        self.packer.grow(edge_u16(self.edge));
        true
    }

    /// Make the texture follow the edge after a grow: one texture at the edge, the old one's
    /// texels copied into its top-left by `encoder`, ahead of whatever the frame writes there.
    /// `true` when the texture moved, so whatever binds its view binds the new one.
    pub(crate) fn commit(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> bool {
        let old = self.texture.width();
        if old == self.edge {
            return false;
        }
        let texture = self.spec.create(device, self.edge);
        encoder.copy_texture_to_texture(
            self.texture.as_image_copy(),
            texture.as_image_copy(),
            wgpu::Extent3d {
                width: old,
                height: old,
                depth_or_array_layers: 1,
            },
        );
        self.view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.texture = texture;
        true
    }

    /// Whether a grow still owes its [`Self::commit`].
    pub(crate) fn has_pending_grow(&self) -> bool {
        self.texture.width() != self.edge
    }

    pub(crate) const fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

/// An edge as the packer counts it; a 2D texture is at most 16384 texels wide.
fn edge_u16(edge: u32) -> u16 {
    u16::try_from(edge).expect("an atlas edge fits a texture's 16-bit edge")
}

/// Whether a rectangle of `size` fits inside a square of `edge` texels.
const fn fits_edge(size: U16Vec2, edge: u32) -> bool {
    size.x as u32 <= edge && size.y as u32 <= edge
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::packed_atlas::{AtlasSpec, PackedAtlas};
    use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
    use glam::U16Vec2;
    use std::iter;
    use std::sync::mpsc;

    /// Every texel of an R8 `texture`, rows top to bottom.
    fn read_r8(gpu: &HeadlessTestGpuLease, texture: &wgpu::Texture) -> Vec<u8> {
        let (w, h) = (texture.width(), texture.height());
        let padded = w.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test.packed_atlas.read"),
            size: u64::from(padded * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            texture.size(),
        );
        gpu.queue.submit(iter::once(encoder.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |mapped| {
            tx.send(mapped).ok();
        });
        gpu.wait();
        rx.recv().unwrap().unwrap();
        let data = slice.get_mapped_range().unwrap();
        data.chunks_exact(padded as usize)
            .flat_map(|line| line[..w as usize].iter().copied())
            .collect()
    }

    /// Two grows in one frame move the packer's edge twice and make one texture, at the final
    /// edge, when the frame commits: 16 to 64, held to the 64 ceiling past it. The old texels
    /// land in its top-left, texel for texel, and the rest is the new texture's zeros; a commit
    /// with no grow moves nothing.
    #[test]
    fn two_grows_make_one_texture_that_keeps_the_texels() {
        let gpu = headless_test_gpu();
        let mut atlas = PackedAtlas::new(
            &gpu.device,
            AtlasSpec {
                label: "test.packed_atlas".to_owned(),
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                ceiling: 64,
            },
            16,
        );
        let texels: Vec<u8> = (0..=255u8).collect();
        gpu.queue.write_texture(
            atlas.texture().as_image_copy(),
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: None,
            },
            atlas.texture().size(),
        );
        assert_eq!(atlas.allocate(U16Vec2::new(16, 16)), Some(U16Vec2::ZERO));
        assert_eq!(
            atlas.allocate(U16Vec2::new(4, 4)),
            None,
            "the 16² edge is full"
        );
        assert!(atlas.grow() && atlas.grow());
        assert!(!atlas.grow(), "at the ceiling");
        assert_eq!(atlas.edge(), 64);
        assert_eq!(atlas.texture().width(), 16, "no texture until the commit");
        assert!(atlas.has_pending_grow());
        assert_eq!(
            atlas.allocate(U16Vec2::new(40, 40)),
            Some(U16Vec2::new(0, 16)),
            "the packer already places on the grown edge"
        );

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        assert!(atlas.commit(&gpu.device, &mut encoder));
        assert!(
            !atlas.commit(&gpu.device, &mut encoder),
            "nothing left to commit"
        );
        gpu.queue.submit(iter::once(encoder.finish()));
        assert_eq!(atlas.texture().width(), 64);

        let read = read_r8(&gpu, atlas.texture());
        for y in 0..64usize {
            for x in 0..64usize {
                let want = if x < 16 && y < 16 {
                    texels[y * 16 + x]
                } else {
                    0
                };
                assert_eq!(read[y * 64 + x], want, "texel ({x}, {y})");
            }
        }
    }
}
