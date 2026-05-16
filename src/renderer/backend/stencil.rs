//! Shared stencil-format constant + the stencil-test depth/stencil
//! state used by every rounded-clip-aware pipeline.
//!
//! The rounded-clip mask is stamped at `stencil_reference = 1` (via
//! `QuadPipeline`'s `mask_write` variant), then every color draw
//! inside the clipped region runs through this state. Sole source of
//! truth so [`QuadPipeline::stencil_test`], `MeshPipeline::stencil_test`,
//! `ImagePipeline::stencil_test`, and glyphon's stencil-aware text
//! renderer all agree on `read_mask`, `compare`, and the face ops —
//! mismatched bits would silently mis-clip text or images under a
//! rounded panel.

/// Format used for the lazy stencil attachment. `Stencil8` is the
/// minimum that satisfies the rounded-clip mask path; no depth
/// component is needed (UI is 2D, no z-test).
pub(super) const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

/// Depth/stencil state for the stencil-test color pipelines (quad /
/// mesh / image / text). Stencil ref is set per-draw by the schedule
/// (`SetStencilRef(0)` outside masks, `1` inside) and compared with
/// `Equal`; `write_mask = 0` keeps the stamped mask intact across the
/// color draws.
pub(super) fn stencil_test_state() -> wgpu::DepthStencilState {
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
