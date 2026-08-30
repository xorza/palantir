//! User-driven GPU rendering: the frontend half of the
//! [`GpuView`](crate::widgets::gpu_view::GpuView) widget. App code implements [`GpuPaint`] on its own renderer (owning
//! whatever pipelines / buffers / depth+MSAA textures it needs), wraps it
//! in `Rc<RefCell<…>>`, and hands it to the widget each frame. The
//! framework owns an off-screen render target sized to the widget's composed
//! physical rect (uniformly downsampled when the device texture cap requires
//! it), runs the callback into it during submit, and composites the result
//! through the existing image pipeline — so clipping, rounded corners, z-order,
//! and partial-damage recompositing come for free.
//!
//! The `Ui` keeps one [`GpuViews`](crate::renderer::gpu_paint::gpu_views::GpuViews)
//! store of live views: the app hands its renderer to the widget every
//! frame, so [`Ui::gpu_view`](crate::Ui::gpu_view) records it there —
//! minting the stable backend
//! [`TextureId`](crate::primitives::texture_id::TextureId) once from
//! `UiResources`' shared authority, so it cannot collide with registered
//! images or other windows, and refreshing the
//! [`GpuPaintRef`](crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef).
//! The shape records only the redraw `epoch`; the encoder looks the view
//! up by the node's `WidgetId`, forwards the callback alongside the image
//! payload, and the composer lists it in `RenderBuffer::frame_targets` for
//! the backend. `Frontend::build` separately fills
//! `RenderBuffer::live_targets` from the whole store — what the frame
//! *recorded*, as against what it *painted* — and that is what the backend
//! keys target retention on, so an unchanged view culled out of a frame keeps
//! its texture. The store is swept by the same `removed` set as every other
//! per-widget cache; the backend then frees the orphaned texture (see
//! `ImageTextures::paint_gpu_views`).

pub(crate) mod gpu_frame_ctx;
pub(crate) mod gpu_init_ctx;
pub(crate) mod gpu_paint_ref;
pub(crate) mod gpu_views;

use crate::renderer::gpu_paint::gpu_frame_ctx::GpuFrameCtx;
use crate::renderer::gpu_paint::gpu_init_ctx::GpuInitCtx;

/// Implemented by app code on its persistent renderer to draw raw `wgpu`
/// content into a [`GpuView`](crate::widgets::gpu_view::GpuView) widget.
/// `'static` because the framework holds the renderer (behind
/// `Rc<RefCell<…>>`) across the whole frame — the render runs at paint
/// time, after `App::record` has returned, so it can't borrow frame-local
/// state.
pub trait GpuPaint: 'static {
    /// Build GPU resources (pipelines, persistent buffers). Called **once**
    /// per view, the first time the device is available for it. Skipping
    /// paints does not re-run it — a view marked
    /// [`repaint(false)`](crate::widgets::gpu_view::GpuView::repaint) keeps its
    /// off-screen texture — and neither does a resize, since the resolved
    /// color target is framework-owned. Recreate your own depth / MSAA
    /// attachments inside [`Self::paint`] when [`GpuFrameCtx::size_px`]
    /// changes.
    ///
    /// It runs again only after the view is genuinely gone and comes back:
    /// the widget stopped being recorded (so its state was swept, like every
    /// other per-widget cache), or its window closed. A renderer that outlives
    /// its widget — one parked in app state across a page switch — is handed
    /// a fresh target when it returns and is initialized into it.
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        let _ = ctx;
    }

    /// Render into the off-screen target. Open your own render pass(es) on
    /// `ctx.encoder` against `ctx.target`; they ride palantir's main submit
    /// and the result is composited into the UI at the widget's rect.
    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>);
}
