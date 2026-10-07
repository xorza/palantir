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

use crate::gpu::device::gpu_frame_context::GpuFrameContext;
use crate::gpu::device::gpu_init_context::GpuInitContext;

/// Implemented by app code on its persistent renderer to draw raw `wgpu`
/// content into a [`GpuView`](crate::widgets::gpu_view::GpuView). `'static`
/// because rendering happens after `App::record` returns.
///
/// **Write premultiplied colour into the target**, as the composite samples
/// it like a registered image.
pub trait GpuPaint: 'static {
    /// Builds GPU resources. Called **once** per view; skipped paints and
    /// resizes do not re-run it, so recreate your depth/MSAA attachments in
    /// [`Self::paint`] when [`GpuFrameContext::physical_size`] changes. It
    /// runs again only after the view is gone (widget unrecorded or window
    /// closed) and returns.
    fn init(&mut self, context: &GpuInitContext<'_>) {
        let _ = context;
    }

    /// Renders into the off-screen target with your own pass(es) on
    /// `context.encoder` against `context.target`.
    fn paint(&mut self, context: &mut GpuFrameContext<'_>);
}
