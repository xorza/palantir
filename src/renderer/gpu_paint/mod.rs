//! User-driven GPU rendering: the frontend half of
//! [`GpuView`](crate::widgets::gpu_view::GpuView). App code implements
//! [`GpuPaint`] on its renderer and hands it to the widget each frame; the
//! framework runs it into an off-screen target and composites it through the
//! image pipeline.
//!
//! The `Ui` keeps a [`GpuViews`](crate::renderer::gpu_paint::gpu_views::GpuViews)
//! store of live views. The shape records only the redraw `epoch`;
//! `Frontend::build` fills `RenderBuffer::live_targets` from the whole store
//! (recorded, not painted) so a culled unchanged view keeps its texture.

pub(crate) mod gpu_paint_ref;
pub(crate) mod gpu_views;

use crate::gpu::device::gpu_frame_ctx::GpuFrameCtx;
use crate::gpu::device::gpu_init_ctx::GpuInitCtx;

/// Implemented by app code on its persistent renderer to draw raw `wgpu`
/// content into a [`GpuView`](crate::widgets::gpu_view::GpuView). `'static`
/// because rendering happens after `App::record` returns.
///
/// **Write premultiplied color into the target**, as the composite samples
/// it like a registered image.
pub trait GpuPaint: 'static {
    /// Builds GPU resources. Called **once** per view; skipped paints and
    /// resizes do not re-run it, so recreate your depth/MSAA attachments in
    /// [`Self::paint`] when the size of [`GpuFrameCtx::target`]'s texture changes. It
    /// runs again after the view is gone (widget unrecorded or window
    /// closed) and returns, and on a new device, after the host recovers from a
    /// lost one: replace everything an earlier call built, as it belongs to the
    /// old device.
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        let _ = ctx;
    }

    /// Renders into the off-screen target with your own pass(es) on
    /// `ctx.encoder` against `ctx.target`.
    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>);
}
