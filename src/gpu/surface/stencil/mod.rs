//! The per-window stencil attachment the rounded-clip pipelines stamp and test, in the format
//! and under the states [`ClipStencil`] gives them.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::stencil_variant::ClipStencil;
use crate::gpu::texel;
use glam::UVec2;

/// A window's stencil attachment for rounded-clip masking, sized to the render target; its
/// [`RenderStream`](crate::gpu::wgpu_backend::render_stream::RenderStream) makes and keeps it.
#[derive(Debug)]
pub(crate) struct Stencil {
    /// Held for its extent, which [`Self::fits`] compares before reuse.
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Stencil {
    /// The attachment view, behind an accessor so a `&Stencil` holder cannot reach the texture.
    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Whether the attachment is `size` physical px.
    pub(crate) fn fits(&self, size: UVec2) -> bool {
        self.texture.size() == texel::extent(size)
    }

    /// A stencil attachment of `size` physical px, its texels written to 0 by a copy before any
    /// pass: on NVIDIA under Vulkan, an `S8_UINT` image bound to memory that held nonzero bytes
    /// keeps 4 × 4 blocks that neither a load clear nor a stencil write resets, and every draw of a
    /// rounded-clip pass then fails there (`.notes/research/2026-10-31-shared-device-tile-loss.md`).
    pub(crate) fn new(gpu: &Gpu, size: UVec2) -> Self {
        let extent = texel::extent(size);
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST;
        #[cfg(test)]
        let usage = usage | wgpu::TextureUsages::COPY_SRC;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.stencil"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ClipStencil::FORMAT,
            usage,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::StencilOnly,
            },
            &vec![0; extent.width as usize * extent.height as usize],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(extent.width),
                rows_per_image: None,
            },
            extent,
        );
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            texture,
        }
    }
}

#[cfg(test)]
mod tests;
