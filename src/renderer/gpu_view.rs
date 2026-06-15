//! User-driven GPU rendering: the frontend half of the [`GpuView`]
//! widget. App code implements [`GpuPaint`] on its own renderer (owning
//! whatever pipelines / buffers / depth+MSAA textures it needs), wraps it
//! in `Rc<RefCell<…>>`, and hands it to the widget each frame. The
//! framework owns an off-screen render target sized to the widget's rect,
//! runs the callback into it during submit, and composites the result
//! through the existing image pipeline — so clipping, rounded corners,
//! z-order, and partial-damage recompositing come for free.
//!
//! The `Ui` keeps one small per-`WidgetId` map of live views (`Ui::gpu_views`,
//! values are [`GpuViewEntry`]): the app hands its renderer to the widget every
//! frame, so [`Ui::gpu_view`] upserts the entry — minting the stable backend
//! [`TextureId`] once (from the shared `texture_ids`, so the one backend
//! texture cache can't collide, including across windows since the map is
//! per-`Ui`) and refreshing the [`GpuPaintRef`]. The shape records only the
//! redraw `epoch`; the encoder looks the view up by the node's `WidgetId`,
//! forwards the callback down the command buffer, and the composer lists it in
//! `RenderBuffer::frame_targets` for the backend. The map is swept by the same
//! `removed` set as every other per-widget cache; the backend then frees the
//! orphaned texture heuristically (see `GpuViewTargets::paint`).

use crate::renderer::texture_id::TextureId;
use glam::UVec2;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// The off-screen target's texture format. `Rgba8UnormSrgb`, identical to
/// registered images, so the image pipeline samples a `GpuView` target
/// exactly like any other texture: the user writes linear in their
/// fragment shader, the format encodes sRGB on store, and palantir's
/// sampler decodes back to linear. App code matches this on its color
/// target (see [`GpuInitCtx::target_format`]).
pub(crate) const GPU_VIEW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Implemented by app code on its persistent renderer to draw raw `wgpu`
/// content into a [`GpuView`](crate::widgets::gpu_view::GpuView) widget.
/// `'static` because the framework holds the renderer (behind
/// `Rc<RefCell<…>>`) across the whole frame — the render runs at paint
/// time, after `App::frame` has returned, so it can't borrow frame-local
/// state.
pub trait GpuPaint: 'static {
    /// Build GPU resources (pipelines, persistent buffers). Called once,
    /// the first time the device is available for this view. Not re-run on
    /// resize — the resolved color target is framework-owned; recreate any
    /// of your own depth / MSAA attachments inside [`Self::paint`] when
    /// [`GpuFrameCtx::size_px`] changes.
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        let _ = ctx;
    }

    /// Render into the off-screen target. Open your own render pass(es) on
    /// `ctx.encoder` against `ctx.target`; they ride palantir's main submit
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
    /// Palantir's main command encoder — record your render pass(es) here.
    /// wgpu inserts the `RENDER_ATTACHMENT → TEXTURE_BINDING` transition
    /// between your pass and the main pass that samples `target`.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The off-screen color target, sized exactly to [`Self::size_px`]. Set
    /// your viewport/scissor to `size_px` and render into the whole target.
    pub target: &'a wgpu::TextureView,
    /// The target's size, in physical pixels (the widget rect × DPI scale).
    /// Set your viewport to this, derive your projection from it, and size
    /// your own attachments (depth, MSAA) to it — the target is reallocated
    /// whenever this changes (every frame while the view is being resized).
    pub size_px: UVec2,
    /// Logical→physical scale factor for this frame.
    pub scale: f32,
    /// Wall-clock time since this view last painted (`Duration::ZERO` on
    /// its first paint). Use it to make animation framerate-independent.
    pub dt: Duration,
}

impl std::fmt::Debug for GpuFrameCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuFrameCtx")
            .field("size_px", &self.size_px)
            .field("scale", &self.scale)
            .field("dt", &self.dt)
            .finish_non_exhaustive()
    }
}

/// The app's `GpuPaint` callback, flowing record → shape → command buffer →
/// `RenderBuffer.frame_targets` → backend. A thin wrapper so the structs that
/// carry it ([`ShapeRecord::GpuView`](crate::forest::shapes::record::ShapeRecord),
/// `RenderTargetDraw`) keep their `derive(Debug)` despite `dyn GpuPaint` not
/// being `Debug`. Clone is an `Rc` refcount bump.
#[derive(Clone)]
pub(crate) struct GpuPaintRef(pub(crate) Rc<RefCell<dyn GpuPaint>>);

impl std::fmt::Debug for GpuPaintRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GpuPaint")
    }
}

/// One live `GpuView` in [`Ui::gpu_views`](crate::ui::Ui), keyed by `WidgetId`:
/// the view's stable backend `texture_id` (minted once from the shared
/// `texture_ids`, so it can't collide in the one backend texture cache — across
/// windows too, since the map is per-`Ui`) and the app `paint` callback
/// (refreshed every frame). This is the only place a `GpuView`'s identity
/// persists across frames; the swept-by-`removed` map is the whole of the
/// `Ui`'s `GpuView` bookkeeping — no `by_texture` index, no drop queue (the
/// backend frees heuristically), no resolve (the composer lists targets).
#[derive(Debug)]
pub(crate) struct GpuViewEntry {
    pub(crate) texture_id: TextureId,
    pub(crate) paint: GpuPaintRef,
}
