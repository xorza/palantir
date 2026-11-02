//! [`TexelReadback`]: a texture's texels copied back whole, through a staging buffer kept for the
//! next copy.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::surface::staging::Staging;
use crate::gpu::texel;
use glam::UVec2;
use std::iter;

/// A staging buffer sized for one texture's texels. Its owner keeps it, so a recording that copies
/// every frame back makes one buffer, not one a frame.
#[derive(Debug)]
pub(crate) struct TexelReadback {
    staging: Staging,
    size: UVec2,
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
        let size = texel::size(texture);
        Self {
            staging: Staging::new(gpu, "palantir.texel_readback", size, texel),
            size,
        }
    }

    /// Every texel of `texture`'s first layer as the texture holds it, copied back and waited for:
    /// `texels` takes the rows top down with no padding between them. Returns how the wait ended;
    /// `texels` is left empty unless [`GpuWait::Done`].
    pub(crate) fn read(&self, gpu: &Gpu, texture: &wgpu::Texture, texels: &mut Vec<u8>) -> GpuWait {
        debug_assert_eq!(
            texel::size(texture),
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
            self.staging.copy_info(),
            texel::extent(self.size),
        );
        gpu.queue.submit(iter::once(encoder.finish()));
        self.staging.map_read();
        let map = self.staging.map();
        match gpu.wait_until(&|| !map.is_pending()) {
            GpuWait::Done if map.is_mapped() => {}
            GpuWait::Done => {
                debug_assert!(gpu.is_lost(), "a map fails only on a lost device");
                return GpuWait::DeviceLost;
            }
            waited => {
                self.staging.abort();
                return waited;
            }
        }
        self.staging.unpad_into(texels);
        GpuWait::Done
    }
}
