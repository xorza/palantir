//! The per-window stencil attachment and the stencil-test state shared by every
//! rounded-clip-aware pipeline. Masks stamp as a depth-counted stack
//! (`mask_stamp`: level `k` writes `k + 1` where the stencil equals `k`), and
//! colour draws test at `stencil_reference = chain depth`.

use crate::gpu::surface::render_target;
use glam::UVec2;

use crate::renderer::render_buffer::MAX_ROUNDED_CLIP_DEPTH;

/// Per-window stencil attachment for rounded-clip masking, created lazily and
/// resized to the render target. Separate from
/// [`Backbuffer`](crate::gpu::surface::backbuffer::Backbuffer); owned by `WindowDriver`.
#[derive(Debug)]
pub(crate) struct Stencil {
    /// Held for its extent, which [`Self::ensure`] compares before reuse.
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Stencil {
    /// `Stencil8` suffices for the mask path; UI is 2D.
    pub(super) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

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

    /// The stencil-test colour pipelines: ref 0 outside masks, chain depth
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

    /// The window's stencil attachment at `size`, built if the slot is empty or
    /// differently sized; returns it like
    /// [`Backbuffer::ensure`](crate::gpu::surface::backbuffer::Backbuffer::ensure).
    pub(crate) fn ensure<'s>(
        slot: &'s mut Option<Self>,
        device: &wgpu::Device,
        size: UVec2,
    ) -> &'s Self {
        let size = render_target::extent(size);
        if slot.as_ref().is_some_and(|held| held.tex.size() != size) {
            *slot = None;
        }
        slot.get_or_insert_with(|| Self::new(device, size))
    }

    /// The attachment view, behind an accessor so a `&Stencil` holder cannot reach the texture.
    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Private: [`Self::ensure`] is the only way to one.
    fn new(device: &wgpu::Device, size: wgpu::Extent3d) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.stencil"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Self {
            view: tex.create_view(&wgpu::TextureViewDescriptor::default()),
            tex,
        }
    }
}
