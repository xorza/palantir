//! The per-window stencil attachment, its format, and the
//! stencil-test depth/stencil state every rounded-clip-aware pipeline
//! shares.
//!
//! The rounded-clip masks stamp as a depth-counted stack (via
//! `QuadPipeline`'s `mask_stamp` variant — chain level `k` writes
//! `k + 1` where the stencil already equals `k`), then every color
//! draw inside the clipped region runs through this state at
//! `stencil_reference = chain depth`. Sole source of truth so the
//! quad / mesh / image / curve stencil-test twins and the
//! stencil-aware text renderer all agree on `read_mask`, `compare`,
//! and the face ops — mismatched bits would silently mis-clip text or
//! images under a rounded panel.

use crate::renderer::render_buffer::MAX_ROUNDED_CLIP_DEPTH;

/// Per-window stencil attachment for rounded-clip masking, allocated lazily on
/// the first rounded-clip frame and resized to match the render target. Kept
/// separate from [`Backbuffer`](crate::renderer::backend::backbuffer::Backbuffer) so the direct-present path can have a stencil
/// without paying for a backbuffer color texture it never uses. Transient:
/// cleared at pass open, never read across frames. Owned per-window by
/// `WindowDriver`.
#[derive(Debug)]
pub(crate) struct Stencil {
    /// Held for its extent, which [`WgpuBackend::ensure_stencil`](crate::renderer::backend::WgpuBackend::ensure_stencil) compares
    /// against the target's before reusing the attachment. The view keeps
    /// the texture alive either way, so this is a handle, not a second
    /// record of a size the texture already knows.
    tex: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

impl Stencil {
    /// Format used for the lazy stencil attachment. `Stencil8` is the
    /// minimum that satisfies the rounded-clip mask path; no depth
    /// component is needed (UI is 2D, no z-test).
    pub(super) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

    /// Depth/stencil state for the stencil-test color pipelines (quad /
    /// mesh / image / text). Stencil ref is set per-draw by the schedule
    /// (`SetStencilRef(0)` outside masks, the chain depth inside) and
    /// compared with `Equal`; `write_mask = 0` keeps the stamped masks
    /// intact across the color draws.
    pub(super) fn test_state() -> wgpu::DepthStencilState {
        let face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Keep,
        };
        wgpu::DepthStencilState {
            format: Self::FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: face,
                back: face,
                read_mask: MAX_ROUNDED_CLIP_DEPTH,
                write_mask: 0x00,
            },
            bias: wgpu::DepthBiasState::default(),
        }
    }

    /// The attachment's extent, which [`WgpuBackend::ensure_stencil`](crate::renderer::backend::WgpuBackend::ensure_stencil)
    /// compares against the target's before reusing it.
    ///
    /// [`WgpuBackend::ensure_stencil`](crate::renderer::backend::WgpuBackend::ensure_stencil):
    ///     crate::renderer::backend::WgpuBackend::ensure_stencil
    pub(super) fn size(&self) -> wgpu::Extent3d {
        self.tex.size()
    }

    /// Private, so [`WgpuBackend::ensure_stencil`](crate::renderer::backend::WgpuBackend::ensure_stencil) is the only way to one.
    pub(super) fn new(device: &wgpu::Device, size: wgpu::Extent3d) -> Self {
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
