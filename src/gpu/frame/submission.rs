//! One frame's worth of work handed to the backend, and the textures it
//! writes into.

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

/// `Copy` because all three are borrows: [`WgpuBackend::submit`] pulls
/// them out up front and still hands the whole [`Submission`] to its
/// upload half.
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
    /// The window's cutout plan for `buffer`'s quads.
    pub(crate) cutouts: &'a CutoutPlan,
    pub(crate) debug_overlay: DebugOverlayConfig,
}

impl Submission<'_> {
    /// Effective clear colour, opaque: the buffer's override where it set
    /// one, else the plan's. The frame's bottom paint layer, so both the
    /// `Full` pass's `LoadOp::Clear` and the `Partial` pre-clear quad read
    /// it. In the `f16` lanes the quad carries, so the two start from one
    /// value and a partial repaint blends over the base a full one does.
    pub(crate) fn clear(&self) -> RgbaF16 {
        let clear = self
            .buffer
            .clear_override
            .map_or(self.plan.clear, RgbaF16::unpack);
        RgbaF16::from(RgbaF32 { a: 1.0, ..clear })
    }

    /// The debug dim flag, and only on a frame it can apply to — a full
    /// repaint has no undamaged region left to dim.
    pub(crate) const fn dim_undamaged(&self) -> bool {
        self.debug_overlay.dim_undamaged && self.plan.damage.is_partial()
    }
}
