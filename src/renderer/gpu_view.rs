//! User-driven GPU rendering: the frontend half of the [`GpuView`]
//! widget. App code implements [`GpuPaint`] on its own renderer (owning
//! whatever pipelines / buffers / depth+MSAA textures it needs), wraps it
//! in `Rc<RefCell<…>>`, and hands it to the widget each frame. The
//! framework owns an off-screen render target sized to the widget's rect,
//! runs the callback into it during submit, and composites the result
//! through the existing image pipeline — so clipping, rounded corners,
//! z-order, and partial-damage recompositing come for free.
//!
//! [`GpuViewRegistry`] is **per-window** (one per [`Ui`](crate::ui::Ui)),
//! keyed by [`WidgetId`] — the same per-widget-state model as
//! [`StateMap`](crate::ui::state): `upsert` on record, `sweep_removed`
//! evicts widgets that vanished this frame. It mints globally-unique
//! [`TextureId`]s from a shared [`TextureIdSource`] (the one the
//! `ImageRegistry` uses), so the **one** backend texture cache — shared
//! across all windows — never collides. Per-window keying is what keeps two
//! windows' views at the same call site (same `WidgetId`) on distinct
//! textures.

use crate::primitives::widget_id::WidgetId;
use crate::renderer::texture_id::{TextureId, TextureIdSource};
use glam::UVec2;
use rustc_hash::{FxHashMap, FxHashSet};
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
    /// The off-screen color target. **May be larger than `size_px`** — the
    /// framework grows it on a √2 ladder and never shrinks it, so a smooth
    /// resize doesn't reallocate every frame. Render into the **top-left
    /// `size_px` sub-rect** (set your viewport/scissor to `size_px`); the
    /// composite samples only that sub-rect. Its full allocated size is
    /// [`Self::target_size`].
    pub target: &'a wgpu::TextureView,
    /// The region to render into, in physical pixels (the widget rect ×
    /// DPI scale). Set your viewport to this and derive your projection
    /// from it.
    pub size_px: UVec2,
    /// Full allocated size of `target` (≥ `size_px`). Size your own
    /// attachments (depth, MSAA) to this so they don't churn on every
    /// resize — they only need recreating when `target_size` changes,
    /// which the ladder makes rare.
    pub target_size: UVec2,
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
            .field("target_size", &self.target_size)
            .field("scale", &self.scale)
            .field("dt", &self.dt)
            .finish_non_exhaustive()
    }
}

/// One widget's live `GpuView`: a stable texture id + the app renderer. The
/// backend iterates [`GpuViewRegistry::entries`] over these to paint.
pub(crate) struct View {
    /// Stable across frames (minted once); keys the backend texture cache.
    pub(crate) id: TextureId,
    pub(crate) paint: Rc<RefCell<dyn GpuPaint>>,
}

/// Per-window registry of live `GpuView`s, keyed by [`WidgetId`]. See the
/// module docs: one per `Ui`, swept by the removed-set, minting from a
/// shared [`TextureIdSource`].
pub(crate) struct GpuViewRegistry {
    ids: TextureIdSource,
    /// Live views by widget; the backend iterates `.values()` each frame to
    /// paint them (`pub(crate)` so it can, without a copy).
    pub(crate) entries: FxHashMap<WidgetId, View>,
    /// `TextureId`s whose widget was swept this frame; the backend frees the
    /// matching texture + bind group on its next reconcile.
    dropped: Vec<TextureId>,
}

impl GpuViewRegistry {
    /// Build a registry minting from `ids` (shared with the `ImageRegistry`
    /// so their ids can't collide in the one backend cache).
    pub(crate) fn new(ids: TextureIdSource) -> Self {
        Self {
            ids,
            entries: FxHashMap::default(),
            dropped: Vec::new(),
        }
    }

    /// Record `widget`'s view this frame, returning its stable [`TextureId`].
    /// Mints the id on first sight and refreshes the renderer each frame.
    /// Re-rendering isn't tracked here — the widget stamps the `Ui` frame
    /// counter as the shape's epoch, so a painted frame always redraws the
    /// view; the app drives frames with [`Ui::request_repaint`].
    pub(crate) fn upsert(
        &mut self,
        widget: WidgetId,
        paint: Rc<RefCell<dyn GpuPaint>>,
    ) -> TextureId {
        let view = self.entries.entry(widget).or_insert_with(|| View {
            // Only runs on first sight — mint the stable id then. (The
            // `paint` here is replaced just below; the update path skips
            // this closure, so it costs nothing per steady-state frame.)
            id: self.ids.reserve(),
            paint: Rc::clone(&paint),
        });
        view.paint = paint;
        view.id
    }

    /// Evict views whose widget wasn't recorded this frame, queueing their
    /// textures for release. Driven from `Ui::post_record` by the same
    /// `removed` set that sweeps `StateMap`.
    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        for w in removed {
            if let Some(view) = self.entries.remove(w) {
                self.dropped.push(view.id);
            }
        }
    }

    /// Drain the ids of swept views, calling `free` for each (the backend
    /// drops the matching texture + bind group). Drains in place.
    pub(crate) fn drain_dropped(&mut self, mut free: impl FnMut(TextureId)) {
        for id in self.dropped.drain(..) {
            free(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct NoopPaint;
    impl GpuPaint for NoopPaint {
        fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
    }

    fn rc() -> Rc<RefCell<dyn GpuPaint>> {
        Rc::new(RefCell::new(NoopPaint))
    }

    fn wid(n: u64) -> WidgetId {
        WidgetId::from_hash(n)
    }

    fn live_ids(reg: &GpuViewRegistry) -> Vec<TextureId> {
        reg.entries.values().map(|v| v.id).collect()
    }

    #[test]
    fn upsert_mints_unique_ids_and_reuses_per_widget() {
        let ids = TextureIdSource::default();
        let mut reg = GpuViewRegistry::new(ids.clone());
        let a = reg.upsert(wid(1), rc());
        let b = reg.upsert(wid(2), rc());
        // Distinct widgets get distinct, nonzero ids.
        assert_ne!(a, b);
        assert_ne!(a.0, 0);
        // The same widget reuses its id across frames (stable texture).
        let a2 = reg.upsert(wid(1), rc());
        assert_eq!(a, a2);
        // An id minted straight from the shared source collides with neither.
        let other = ids.reserve();
        assert_ne!(other, a);
        assert_ne!(other, b);
    }

    #[test]
    fn entries_list_live_views() {
        let mut reg = GpuViewRegistry::new(TextureIdSource::default());
        let a = reg.upsert(wid(1), rc());
        let b = reg.upsert(wid(2), rc());
        let ids = live_ids(&reg);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&a));
        assert!(ids.contains(&b));
    }

    #[test]
    fn sweep_removed_evicts_and_queues_release() {
        let mut reg = GpuViewRegistry::new(TextureIdSource::default());
        let id = reg.upsert(wid(1), rc());
        // Sweeping an unrelated widget leaves it live, nothing freed.
        reg.sweep_removed(&FxHashSet::from_iter([wid(2)]));
        assert_eq!(live_ids(&reg).len(), 1);
        let mut freed = Vec::new();
        reg.drain_dropped(|id| freed.push(id));
        assert!(freed.is_empty());
        // Sweeping its own widget evicts it + queues the texture once.
        reg.sweep_removed(&FxHashSet::from_iter([wid(1)]));
        assert!(
            live_ids(&reg).is_empty(),
            "no live view after its widget is swept"
        );
        reg.drain_dropped(|id| freed.push(id));
        assert_eq!(freed, vec![id]);
        reg.drain_dropped(|id| freed.push(id));
        assert_eq!(freed, vec![id], "drain consumes dropped");
    }
}
