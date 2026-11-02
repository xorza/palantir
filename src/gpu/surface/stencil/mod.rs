//! The per-window stencil attachment and the stencil-test state shared by every
//! rounded-clip-aware pipeline. Masks stamp as a depth-counted stack
//! (`mask_stamp`: level `k` writes `k + 1` where the stencil equals `k`), and
//! color draws test at `stencil_reference = chain depth`.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::render_target;
use glam::UVec2;

use crate::renderer::render_buffer::MAX_ROUNDED_CLIP_DEPTH;

/// A window's stencil attachment for rounded-clip masking, sized to the render target; its
/// [`RenderStream`](crate::gpu::wgpu_backend::render_stream::RenderStream) makes and keeps it.
#[derive(Debug)]
pub(crate) struct Stencil {
    /// Held for its extent, which [`Self::fits`] compares before reuse.
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Stencil {
    /// `Stencil8` suffices for the mask path; UI is 2D.
    pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

    /// Depth/stencil state for one rounded-clip pipeline; only `compare`,
    /// `pass_op` and `write_mask` differ, so the rest is stated once.
    fn state(
        compare: wgpu::CompareFunction,
        pass_op: wgpu::StencilOperation,
        write_mask: u32,
    ) -> wgpu::DepthStencilState {
        let face = wgpu::StencilFaceState {
            compare,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op,
        };
        wgpu::DepthStencilState {
            format: Self::FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: face,
                back: face,
                read_mask: MAX_ROUNDED_CLIP_DEPTH,
                write_mask,
            },
            bias: wgpu::DepthBiasState::default(),
        }
    }

    /// The stencil-test color pipelines: ref 0 outside masks, chain depth
    /// inside, compared `Equal`; `write_mask = 0` keeps the stamped masks.
    pub(crate) fn test_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Equal,
            wgpu::StencilOperation::Keep,
            0x00,
        )
    }

    /// The mask-stamp variant, drawn per level at `stencil_reference = level`:
    /// writes `level + 1` where the SDF passes and the stencil equals `level`.
    pub(crate) fn stamp_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Equal,
            wgpu::StencilOperation::IncrementClamp,
            MAX_ROUNDED_CLIP_DEPTH,
        )
    }

    /// The mask-clear variant, drawn at reference 0; the outermost quad suffices.
    pub(crate) fn clear_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Always,
            wgpu::StencilOperation::Replace,
            MAX_ROUNDED_CLIP_DEPTH,
        )
    }

    /// The attachment view, behind an accessor so a `&Stencil` holder cannot reach the texture.
    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Whether the attachment is `size` physical px.
    pub(crate) fn fits(&self, size: UVec2) -> bool {
        self.texture.size() == render_target::extent(size)
    }

    /// A stencil attachment of `size` physical px, its texels written to 0 by a copy before any
    /// pass: on NVIDIA under Vulkan, an `S8_UINT` image bound to memory that held nonzero bytes
    /// keeps 4 × 4 blocks that neither a load clear nor a stencil write resets, and every draw of a
    /// rounded-clip pass then fails there (`.notes/research/2026-10-31-shared-device-tile-loss.md`).
    pub(crate) fn new(gpu: &Gpu, size: UVec2) -> Self {
        let extent = render_target::extent(size);
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST;
        #[cfg(test)]
        let usage = usage | wgpu::TextureUsages::COPY_SRC;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.stencil"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
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
