//! Shared stencil-format constant + the stencil-test depth/stencil
//! state used by every rounded-clip-aware pipeline.
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

/// Format used for the lazy stencil attachment. `Stencil8` is the
/// minimum that satisfies the rounded-clip mask path; no depth
/// component is needed (UI is 2D, no z-test).
pub(crate) const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

/// Depth/stencil state for the stencil-test color pipelines (quad /
/// mesh / image / text). Stencil ref is set per-draw by the schedule
/// (`SetStencilRef(0)` outside masks, the chain depth inside) and
/// compared with `Equal`; `write_mask = 0` keeps the stamped masks
/// intact across the color draws.
pub(crate) fn stencil_test_state() -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Equal,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Keep,
    };
    wgpu::DepthStencilState {
        format: STENCIL_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Always),
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: 0x00,
        },
        bias: wgpu::DepthBiasState::default(),
    }
}
