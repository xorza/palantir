//! The rounded-clip stencil's format and the states every rounded-clip-aware pipeline takes.
//! Masks stamp as a depth-counted stack (`stamp_state`: level `k` writes `k + 1` where the
//! stencil equals `k`), and color draws test at `stencil_reference = chain depth`. The window's
//! attachment, [`Stencil`](crate::gpu::surface::stencil::Stencil), is made in its format.

use crate::renderer::render_buffer::MAX_ROUNDED_CLIP_DEPTH;

/// `Stencil8` suffices for the mask path; UI is 2D.
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

/// Every bit of [`FORMAT`], which the masks read and write. A depth is a count, not a mask: the
/// deepest stack must fit the format, and a shallower limit keeps the full mask.
const MASK: u32 = 0xFF;

const _: () = assert!(
    MAX_ROUNDED_CLIP_DEPTH <= MASK,
    "the deepest rounded-clip stack fits the stencil"
);

/// Depth/stencil state for one rounded-clip pipeline; only `compare`, `pass_op` and `write_mask`
/// differ, so the rest is stated once.
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
        format: FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Always),
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: MASK,
            write_mask,
        },
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The stencil-test color pipelines: ref 0 outside masks, chain depth inside, compared `Equal`;
/// `write_mask = 0` keeps the stamped masks.
pub(crate) fn test_state() -> wgpu::DepthStencilState {
    state(
        wgpu::CompareFunction::Equal,
        wgpu::StencilOperation::Keep,
        0x00,
    )
}

/// The mask-stamp variant, drawn per level at `stencil_reference = level`: writes `level + 1`
/// where the SDF passes and the stencil equals `level`.
pub(crate) fn stamp_state() -> wgpu::DepthStencilState {
    state(
        wgpu::CompareFunction::Equal,
        wgpu::StencilOperation::IncrementClamp,
        MASK,
    )
}

/// The mask-clear variant, drawn at reference 0; the outermost quad suffices.
pub(crate) fn clear_state() -> wgpu::DepthStencilState {
    state(
        wgpu::CompareFunction::Always,
        wgpu::StencilOperation::Replace,
        MASK,
    )
}
