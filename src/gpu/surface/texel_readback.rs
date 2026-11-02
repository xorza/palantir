//! [`TexelReadback`]: a texture's texels copied back whole, through a staging buffer kept for the
//! next copy.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::surface::map_state::MapState;
use crate::gpu::surface::render_target;
use glam::UVec2;
use std::iter;

/// A staging buffer sized for one texture's texels. Its owner keeps it, so a recording that copies
/// every frame back makes one buffer, not one a frame.
#[derive(Debug)]
pub(crate) struct TexelReadback {
    buffer: wgpu::Buffer,
    size: UVec2,
    /// One row's bytes.
    row: u32,
    /// A row's stride in the buffer: whole `COPY_BYTES_PER_ROW_ALIGNMENT` units.
    padded_row: u32,
    map: MapState,
}

impl TexelReadback {
    /// A staging buffer for `texture`'s first layer.
    ///
    /// # Panics
    ///
    /// For a texture made without `COPY_SRC`, or in a format with blocks of more than one texel
    /// or of more than one aspect.
    pub(crate) fn new(gpu: &Gpu, texture: &wgpu::Texture) -> Self {
        let format = texture.format();
        assert!(
            texture.usage().contains(wgpu::TextureUsages::COPY_SRC),
            "a copy reads a texture made with COPY_SRC"
        );
        assert_eq!(
            format.block_dimensions(),
            (1, 1),
            "a texel copy of a {format:?} texture"
        );
        let texel = format
            .block_copy_size(None)
            .expect("a color format has one aspect");
        let size = render_target::texture_size(texture);
        let row = size.x * texel;
        let padded_row = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.texel_readback"),
            size: u64::from(padded_row) * u64::from(size.y),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            size,
            row,
            padded_row,
            map: MapState::new(),
        }
    }

    /// Every texel of `texture`'s first layer as the texture holds it, copied back and waited for:
    /// `texels` takes the rows top down with no padding between them. Returns how the wait ended;
    /// `texels` is left empty unless [`GpuWait::Done`].
    pub(crate) fn read(&self, gpu: &Gpu, texture: &wgpu::Texture, texels: &mut Vec<u8>) -> GpuWait {
        debug_assert_eq!(
            render_target::texture_size(texture),
            self.size,
            "a readback reads the texture it was made for"
        );
        texels.clear();
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.texel_readback"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.size.y),
                },
            },
            render_target::extent(self.size),
        );
        gpu.queue.submit(iter::once(encoder.finish()));
        let map = &self.map;
        map.map_read(&self.buffer);
        match gpu.wait_until(&|| !map.is_pending()) {
            GpuWait::Done if map.is_mapped() => {}
            GpuWait::Done => {
                debug_assert!(gpu.is_lost(), "a map fails only on a lost device");
                return GpuWait::DeviceLost;
            }
            waited => {
                // Aborts the map, so the next read can map the buffer again.
                if map.is_pending() {
                    self.buffer.unmap();
                }
                return waited;
            }
        }
        {
            let view = self
                .buffer
                .slice(..)
                .get_mapped_range()
                .expect("a buffer that mapped reads whole");
            texels.reserve_exact((self.row * self.size.y) as usize);
            for line in view.chunks_exact(self.padded_row as usize) {
                texels.extend_from_slice(&line[..self.row as usize]);
            }
        }
        self.buffer.unmap();
        GpuWait::Done
    }
}
