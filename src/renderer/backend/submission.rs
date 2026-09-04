//! One frame's worth of work handed to the backend, and the textures it
//! writes into.

use crate::diagnostics::DebugOverlayConfig;
use crate::primitives::color::RgbaF32;
use crate::renderer::backend::backbuffer::Backbuffer;
use crate::renderer::backend::stencil::Stencil;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::record_store::RecordStore;

/// `Copy` because all three are borrows: [`WgpuBackend::submit`] pulls
/// them out up front and still hands the whole [`Submission`] to its
/// upload half.
///
/// [`WgpuBackend::submit`]: crate::renderer::backend::WgpuBackend::submit
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubmissionTargets<'a> {
    pub(crate) surface: &'a wgpu::Texture,
    pub(crate) backbuffer: Option<&'a Backbuffer>,
    pub(crate) stencil: Option<&'a Stencil>,
}

#[derive(Debug)]
pub(crate) struct Submission<'a> {
    pub(crate) owner: RenderOwnerId,
    pub(crate) targets: SubmissionTargets<'a>,
    pub(crate) store: &'a RecordStore,
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) plan: RenderPlan,
    pub(crate) debug_overlay: DebugOverlayConfig,
}

impl Submission<'_> {
    /// Effective clear colour: the buffer's override where it set one,
    /// else the plan's. The frame's bottom paint layer, so both the
    /// `Full` pass's `LoadOp::Clear` and the `Partial` pre-clear quad
    /// read it.
    pub(crate) fn clear(&self) -> RgbaF32 {
        self.buffer.clear_override.unwrap_or(self.plan.clear)
    }

    /// The debug dim flag, and only on a frame it can apply to — a full
    /// repaint has no undamaged region left to dim.
    pub(crate) fn dim_undamaged(&self) -> bool {
        self.debug_overlay.dim_undamaged && self.plan.damage.is_partial()
    }
}
