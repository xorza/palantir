//! The `Ui`'s live-`GpuView` bookkeeping, and the per-view row it keeps.

use crate::primitives::texture_id::TextureId;
use crate::primitives::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use std::collections::hash_map::Entry;

/// One live `GpuView`, keyed by `WidgetId`: the view's stable backend
/// `texture_id` (minted once from the shared render caches, so it cannot
/// collide with images or another window), the app `paint` callback
/// (refreshed every frame), and the redraw `epoch`.
#[derive(Debug)]
pub(crate) struct GpuViewEntry {
    pub(crate) texture_id: TextureId,
    pub(crate) paint: GpuPaintRef,
    /// The shape `epoch` stamped on each recorded frame. Bumped to the current
    /// frame id only when the widget requests a repaint; held stable otherwise,
    /// so a static view's shape hash doesn't change and the damage diff treats
    /// it as unchanged (the encoder then culls it, skipping its GPU paint).
    epoch: u64,
}

/// Every `GpuView` the `Ui` has seen and not yet swept. The only place a
/// view's identity persists across frames — no `by_texture` index and no
/// resolve, since the composer lists the targets to paint and the backend
/// frees each the frame it is no longer composited.
#[derive(Debug, Default)]
pub(crate) struct GpuViews {
    entries: WidgetIdMap<GpuViewEntry>,
}

impl GpuViews {
    /// Upsert `id`'s row for this frame and hand back the `epoch` its
    /// shape must carry.
    ///
    /// `repaint` is the widget's per-frame dirty flag. When set, the epoch
    /// bumps to `frame`, so the shape hash changes and the view repaints;
    /// when clear, the epoch is held stable, so the damage diff treats the
    /// view as unchanged and the encoder culls it (skipping its GPU paint
    /// and reusing last frame's pixels). First sight always paints — the
    /// texture does not exist yet.
    ///
    /// **A different callback is a different view**, and takes a fresh
    /// `TextureId` for it. The id is stable for as long as one callback
    /// answers to the widget, which is what the backend's target — and
    /// the `GpuPaint::init` it ran once against that target — is keyed
    /// on. Handing a *new* callback the old target would call `paint` on
    /// something never initialized, and under `repaint(false)` would
    /// leave it the old callback's pixels. A fresh id is a fresh target:
    /// uninitialized, empty, and freed the frame the old one leaves the
    /// live roster.
    pub(crate) fn record(
        &mut self,
        id: WidgetId,
        paint: GpuPaintRef,
        repaint: bool,
        frame: u64,
    ) -> u64 {
        match self.entries.entry(id) {
            Entry::Occupied(e) => {
                let entry = e.into_mut();
                let replaced = entry.paint != paint;
                entry.paint = paint;
                if replaced {
                    entry.texture_id = TextureId::reserve();
                }
                // A replacement always paints, whatever the widget asked
                // for: the fresh target has nothing in it.
                if replaced || repaint {
                    entry.epoch = frame;
                }
                entry.epoch
            }
            Entry::Vacant(e) => {
                e.insert(GpuViewEntry {
                    texture_id: TextureId::reserve(),
                    paint,
                    epoch: frame,
                })
                .epoch
            }
        }
    }

    /// The row the encoder composites for `id`.
    pub(crate) fn view(&self, id: WidgetId) -> &GpuViewEntry {
        &self.entries[&id]
    }

    /// Drop the rows of widgets the frame stopped recording. The backend
    /// frees each orphaned texture the next frame it is absent from the
    /// retention roster.
    pub(crate) fn sweep_removed(&mut self, removed: &WidgetIdSet) {
        self.entries.retain(|id, _| !removed.contains(id));
    }

    /// Fill `out` with the retention roster: every view the frame
    /// *recorded*, as against the `frame_targets` the frame *painted*,
    /// which the damage diff culls an unchanged view out of. Keyed on
    /// that alone, the backend could not tell "unchanged" from "gone" and
    /// would free a live view's target.
    ///
    /// Sorted so the backend's retention sweep can search it instead of
    /// scanning it once per retained target — the product of the two
    /// counts, every submit, where a graph view holds one target per
    /// node. A map's `values()` has no order of its own, so this also
    /// stops the roster from depending on hash order.
    pub(crate) fn collect_live_targets(&self, out: &mut Vec<TextureId>) {
        out.clear();
        out.reserve_exact(self.entries.len());
        out.extend(self.entries.values().map(|view| view.texture_id));
        out.sort_unstable();
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::gpu_frame_ctx::GpuFrameCtx;
    use crate::primitives::widget_id::WidgetId;
    use crate::renderer::gpu_paint::GpuPaint;
    use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
    use crate::renderer::gpu_paint::gpu_views::GpuViews;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct NoopPaint;
    impl GpuPaint for NoopPaint {
        fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
    }

    fn renderer() -> GpuPaintRef {
        GpuPaintRef(Rc::new(RefCell::new(NoopPaint)))
    }

    /// One callback keeps one target, and a different one takes its own.
    ///
    /// `GpuPaint::init` runs once per target, so a target the backend
    /// already initialized would hand a *new* callback a `paint` it never
    /// prepared for — and, where the widget asked for no repaint, the
    /// pixels the old callback left. Identity is the only thing that can
    /// tell the two apart, since the widget id cannot.
    #[test]
    fn a_replaced_callback_takes_a_fresh_target() {
        let id = WidgetId::from_hash("view");
        let mut views = GpuViews::default();
        let first = renderer();

        views.record(id, first.clone(), true, 1);
        let target = views.view(id).texture_id;

        // The same callback, holding still: same target, and the epoch
        // stays where a static view left it.
        let epoch = views.record(id, first.clone(), false, 2);
        assert_eq!(
            views.view(id).texture_id,
            target,
            "one callback, one target"
        );
        assert_eq!(epoch, 1, "and no repaint it did not ask for");

        // A different one: its own target, and a paint whether or not it
        // asked, because that target is empty.
        let epoch = views.record(id, renderer(), false, 3);
        assert_ne!(
            views.view(id).texture_id,
            target,
            "a new callback must not inherit a target init already ran against",
        );
        assert_eq!(epoch, 3, "and it paints on the frame it arrived");
    }
}
