//! One frame of work handed to the backend, and the textures it writes into.

use crate::diagnostics::DebugOverlayConfig;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutPlan;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::surface::stencil::Stencil;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::record_store::RecordStore;

/// `Copy` because all three are borrows: [`WgpuBackend::submit`] pulls them out and still hands the whole [`Submission`] on.
///
/// [`WgpuBackend::submit`]: crate::gpu::wgpu_backend::WgpuBackend::submit
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubmissionTargets<'a> {
    pub(crate) surface: RenderTarget<'a>,
    pub(crate) backbuffer: Option<&'a Backbuffer>,
    pub(crate) stencil: Option<&'a Stencil>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Submission<'a> {
    pub(crate) owner: RenderOwnerId,
    pub(crate) targets: SubmissionTargets<'a>,
    pub(crate) store: &'a RecordStore,
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) plan: RenderPlan,
    pub(crate) cutouts: &'a CutoutPlan,
    pub(crate) debug_overlay: DebugOverlayConfig,
}

impl Submission<'_> {
    /// Effective opaque clear colour: the buffer's override, else the plan's.
    pub(crate) fn clear(&self) -> RgbaF16 {
        let clear = self
            .buffer
            .clear_override
            .map_or(self.plan.clear, RgbaF16::unpack);
        RgbaF16::from(RgbaF32 { a: 1.0, ..clear })
    }

    /// The debug dim flag, only on frames with an undamaged region.
    pub(crate) const fn dim_undamaged(&self) -> bool {
        self.debug_overlay.dim_undamaged && self.plan.damage.is_partial()
    }
}
