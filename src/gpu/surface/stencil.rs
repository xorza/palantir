//! The per-window stencil attachment and the stencil-test state shared by every
//! rounded-clip-aware pipeline. Masks stamp as a depth-counted stack
//! (`mask_stamp`: level `k` writes `k + 1` where the stencil equals `k`), and
//! color draws test at `stencil_reference = chain depth`.

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

    /// A stencil attachment of `size` physical px.
    pub(crate) fn new(device: &wgpu::Device, size: UVec2) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.stencil"),
            size: render_target::extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            texture,
        }
    }
}
