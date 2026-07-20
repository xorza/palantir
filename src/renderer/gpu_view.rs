//! User-driven GPU rendering: the frontend half of the [`GpuView`]
//! widget. App code implements [`GpuPaint`] on its own renderer (owning
//! whatever pipelines / buffers / depth+MSAA textures it needs), wraps it
//! in `Rc<RefCell<…>>`, and hands it to the widget each frame. The
//! framework owns an off-screen render target sized to the widget's composed
//! physical rect (uniformly downsampled when the device texture cap requires
//! it), runs the callback into it during submit, and composites the result
//! through the existing image pipeline — so clipping, rounded corners, z-order,
//! and partial-damage recompositing come for free.
//!
//! The `Ui` keeps one small per-`WidgetId` map of live views (`Ui::gpu_views`,
//! values are [`GpuViewEntry`]): the app hands its renderer to the widget every
//! frame, so [`Ui::gpu_view`] upserts the entry — minting the stable backend
//! [`TextureId`] once from `UiResources`' shared authority, so it cannot
//! collide with registered images or other
//! windows, and refreshing the [`GpuPaintRef`]. The shape records only the
//! redraw `epoch`; the encoder looks the view up by the node's `WidgetId`,
//! forwards the callback down the command buffer, and the composer lists it in
//! `RenderBuffer::frame_targets` for the backend. The map is swept by the same
//! `removed` set as every other per-widget cache; the backend then frees the
//! orphaned texture (see `ImagePipeline::paint_gpu_views`).

use crate::renderer::texture_id::TextureId;
use glam::UVec2;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Implemented by app code on its persistent renderer to draw raw `wgpu`
/// content into a [`GpuView`](crate::widgets::gpu_view::GpuView) widget.
/// `'static` because the framework holds the renderer (behind
/// `Rc<RefCell<…>>`) across the whole frame — the render runs at paint
/// time, after `App::record` has returned, so it can't borrow frame-local
/// state.
pub trait GpuPaint: 'static {
    /// Build GPU resources (pipelines, persistent buffers). Called the first
    /// time the device is available for this view, and **again** if the view's
    /// off-screen texture is later reclaimed and rebuilt — which happens when a
    /// frame is forced by other widgets while this view is marked
    /// [`repaint(false)`](crate::widgets::gpu_view::GpuView::repaint) (it's
    /// culled, so its target is freed). Guard expensive one-time setup against
    /// re-entry (e.g. `if self.pipeline.is_none()`). Not re-run merely on
    /// resize — the resolved color target is framework-owned; recreate any of
    /// your own depth / MSAA attachments inside [`Self::paint`] when
    /// [`GpuFrameCtx::size_px`] changes.
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        let _ = ctx;
    }

    /// Render into the off-screen target. Open your own render pass(es) on
    /// `ctx.encoder` against `ctx.target`; they ride aperture's main submit
    /// and the result is composited into the UI at the widget's rect.
    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>);
}

/// Handed to [`GpuPaint::init`]. Carries only what's needed to build
/// format-dependent pipelines.
#[derive(Debug)]
pub struct GpuInitCtx<'a> {
    pub device: &'a wgpu::Device,
    /// The off-screen color target's format (sRGB `Rgba8UnormSrgb`). Match
    /// it on your render pipeline's color target.
    pub target_format: wgpu::TextureFormat,
}

/// Handed to [`GpuPaint::paint`] each painted frame.
pub struct GpuFrameCtx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    /// Aperture's main command encoder — record your render pass(es) here.
    /// wgpu inserts the `RENDER_ATTACHMENT → TEXTURE_BINDING` transition
    /// between your pass and the main pass that samples `target`.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The off-screen color target, sized exactly to [`Self::size_px`]. Set
    /// your viewport/scissor to `size_px` and render into the whole target.
    pub target: &'a wgpu::TextureView,
    /// The target's actual size in physical pixels, after the widget's composed
    /// transform and any uniform downsampling required by the device texture
    /// cap. Set your viewport to this, derive your projection from it, and size
    /// your own attachments (depth, MSAA) to it — the target is reallocated
    /// whenever this changes (every frame while the view is being resized).
    pub size_px: UVec2,
    /// Logical→display scale for this window's current monitor. This is the
    /// display pixel density only; widget transforms and target downsampling do
    /// not affect it.
    pub display_scale: f32,
    /// Logical→target scale for this view, including the display scale,
    /// composed transforms, and any uniform device-cap downsampling.
    pub raster_scale: f32,
    /// Wall-clock time since this view last painted (`Duration::ZERO` on
    /// its first paint). Use it to make animation framerate-independent.
    pub dt: Duration,
}

impl std::fmt::Debug for GpuFrameCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuFrameCtx")
            .field("size_px", &self.size_px)
            .field("display_scale", &self.display_scale)
            .field("raster_scale", &self.raster_scale)
            .field("dt", &self.dt)
            .finish_non_exhaustive()
    }
}

/// The app's `GpuPaint` callback, flowing [`Ui::gpu_views`](crate::ui::Ui) →
/// command-buffer side-list → `RenderBuffer.frame_targets` → backend (the shape
/// itself carries only an epoch). A thin wrapper so the structs that carry it
/// ([`GpuViewEntry`], `RenderTargetDraw`) keep their `derive(Debug)` despite
/// `dyn GpuPaint` not being `Debug`. Clone is an `Rc` refcount bump.
#[derive(Clone)]
pub(crate) struct GpuPaintRef(pub(crate) Rc<RefCell<dyn GpuPaint>>);

impl std::fmt::Debug for GpuPaintRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GpuPaint")
    }
}

/// One live `GpuView` in [`Ui::gpu_views`](crate::ui::Ui), keyed by `WidgetId`:
/// the view's stable backend `texture_id` (minted once from the shared render
/// caches, so it cannot collide with images or another window), the app
/// `paint` callback (refreshed
/// every frame), and the redraw `epoch`. This is the only place a `GpuView`'s
/// identity persists across frames; the swept-by-`removed` map is the whole of
/// the `Ui`'s `GpuView` bookkeeping — no `by_texture` index, no resolve (the
/// composer lists targets to paint, the backend frees each the frame it's no
/// longer composited).
#[derive(Debug)]
pub(crate) struct GpuViewEntry {
    pub(crate) texture_id: TextureId,
    pub(crate) paint: GpuPaintRef,
    /// The shape `epoch` stamped on each recorded frame. Bumped to the current
    /// frame id only when the widget requests a repaint; held stable otherwise,
    /// so a static view's shape hash doesn't change and the damage diff treats
    /// it as unchanged (the encoder then culls it, skipping its GPU paint).
    pub(crate) epoch: u64,
}
