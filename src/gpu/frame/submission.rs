//! One frame of work handed to the backend, and the textures it writes into.

use crate::diagnostics::DebugOverlayConfig;
use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutPlan;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::surface::stencil::Stencil;
use crate::gpu::surface::viewport::RepaintScissors;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::render_plan::RenderPlan;
use crate::shape::record_store::RecordStore;

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
    /// Where the owner's GPU timings publish.
    pub(crate) stats: &'a GpuPassStats,
    pub(crate) targets: SubmissionTargets<'a>,
    pub(crate) store: &'a RecordStore,
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) plan: RenderPlan,
    /// What [`Self::plan`] repaints inside, which the CPU half fitted to the draw list.
    pub(crate) scissors: &'a RepaintScissors,
    pub(crate) cutouts: &'a CutoutPlan,
    pub(crate) debug_overlay: DebugOverlayConfig,
}

impl Submission<'_> {
    /// Effective opaque clear color: the buffer's override, else the plan's.
    pub(crate) fn clear(&self) -> RgbaF16 {
        let clear = self
            .buffer
            .clear_override
            .map_or(self.plan.clear, RgbaF32::from);
        RgbaF32 { a: 1.0, ..clear }.to_f16()
    }

    /// The debug dim flag, only on frames with an undamaged region.
    pub(crate) const fn dim_undamaged(&self) -> bool {
        self.debug_overlay.dim_undamaged && self.plan.damage.is_partial()
    }
}
