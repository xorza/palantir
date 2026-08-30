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
    /// Held for its extent, which [`Self::ensure`] compares against the
    /// target's before reusing the attachment. The view keeps the
    /// texture alive either way, so this is a handle, not a second
    /// record of a size the texture already knows.
    tex: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

impl Stencil {
    /// Format used for the lazy stencil attachment. `Stencil8` is the
    /// minimum that satisfies the rounded-clip mask path; no depth
    /// component is needed (UI is 2D, no z-test).
    pub(super) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

    /// Depth/stencil state for one rounded-clip pipeline.
    ///
    /// `compare`, `pass_op` and `write_mask` are the whole difference
    /// between the three states below. `fail_op` and `depth_fail_op` are
    /// `Keep` throughout — a fragment that fails the test writes nothing
    /// — the two faces always match, and there is no depth component to
    /// vary. Stating the rest once is what keeps a stamp and the test
    /// that reads it agreeing on which bits are the chain.
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

    /// The stencil-test color pipelines (quad / mesh / image / curve /
    /// text). Stencil ref is set per-draw by the schedule
    /// (`SetStencilRef(0)` outside masks, the chain depth inside) and
    /// compared with `Equal`; `write_mask = 0` keeps the stamped masks
    /// intact across the color draws.
    pub(super) fn test_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Equal,
            wgpu::StencilOperation::Keep,
            0x00,
        )
    }

    /// The mask-stamp variant, drawn once per chain level at
    /// `stencil_reference = level`: writes `level + 1` only where the
    /// SDF passes AND the stencil already equals `level`. So the
    /// outermost mask stamps ref 0 onto the cleared stencil and each
    /// inner one deepens only inside its ancestors, which is what makes
    /// nested masks intersect.
    pub(super) fn stamp_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Equal,
            wgpu::StencilOperation::IncrementClamp,
            MAX_ROUNDED_CLIP_DEPTH,
        )
    }

    /// The mask-clear variant, drawn at `stencil_reference = 0` to reset
    /// a stamped chain. One draw of the chain's *outermost* quad
    /// suffices: inner stamps only ever incremented inside the outer's
    /// SDF, so every nonzero stencil pixel lies under it.
    pub(super) fn clear_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Always,
            wgpu::StencilOperation::Replace,
            MAX_ROUNDED_CLIP_DEPTH,
        )
    }

    /// The window's stencil attachment at `size`, building it if the
    /// slot is empty or holds a differently-sized one. Lazily created on
    /// the first rounded-clip frame and recreated when the render
    /// target's size changes — a mismatched-size attachment fails wgpu
    /// validation.
    ///
    /// Hands the attachment back rather than only filling the slot, on
    /// the terms
    /// [`Backbuffer::ensure`](crate::renderer::backend::backbuffer::Backbuffer::ensure)
    /// states.
    pub(crate) fn ensure<'s>(
        slot: &'s mut Option<Self>,
        device: &wgpu::Device,
        size: wgpu::Extent3d,
    ) -> &'s Self {
        if slot.as_ref().is_some_and(|held| held.tex.size() != size) {
            *slot = None;
        }
        slot.get_or_insert_with(|| Self::new(device, size))
    }

    /// Private, so [`Self::ensure`] is the only way to one.
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
