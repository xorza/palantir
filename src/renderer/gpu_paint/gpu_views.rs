//! The `Ui`'s live-`GpuView` bookkeeping.

use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use std::collections::hash_map::Entry;

/// One live `GpuView` keyed by `WidgetId`: its stable `texture_id`, the app `paint` callback and the redraw `epoch`.
#[derive(Debug)]
pub(crate) struct GpuViewEntry {
    pub(crate) texture_id: TextureId,
    pub(crate) paint: GpuPaintRef,
    /// The shape `epoch`; bumped only on a repaint request so a static view's hash is stable and the encoder culls it.
    epoch: u64,
}

/// Whether a `GpuView`'s content changed this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewContent {
    /// Stamp the frame as the epoch, so the view paints.
    Changed,
    /// Hold the epoch the view last painted at, so the encoder culls it.
    Unchanged,
}

/// One `GpuView` as a frame records it.
#[derive(Debug)]
pub(crate) struct RecordedView {
    pub(crate) id: WidgetId,
    pub(crate) paint: GpuPaintRef,
    pub(crate) content: ViewContent,
}

/// Every `GpuView` the `Ui` has seen and not yet swept.
#[derive(Debug, Default)]
pub(crate) struct GpuViews {
    entries: WidgetIdMap<GpuViewEntry>,
}

impl GpuViews {
    /// Upserts the view's row and returns the `epoch` its shape carries: bumped
    /// to `frame` on [`ViewContent::Changed`], else held so the encoder culls the
    /// view. First sight always paints. A different callback is a different view
    /// and takes a fresh `TextureId`, as the backend target and its
    /// `GpuPaint::init` are keyed on the id.
    pub(crate) fn record(&mut self, view: RecordedView, frame: u64) -> u64 {
        let RecordedView { id, paint, content } = view;
        match self.entries.entry(id) {
            Entry::Occupied(e) => {
                let entry = e.into_mut();
                let replaced = entry.paint != paint;
                entry.paint = paint;
                if replaced {
                    entry.texture_id = TextureId::reserve();
                }
                if replaced || content == ViewContent::Changed {
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

    pub(crate) fn view(&self, id: WidgetId) -> &GpuViewEntry {
        &self.entries[&id]
    }

    /// Drops the rows of widgets the frame stopped recording.
    pub(crate) fn sweep_removed(&mut self, removed: &WidgetIdSet) {
        #[expect(
            clippy::iter_over_hash_type,
            reason = "each row is dropped alone, so the order changes nothing"
        )]
        for id in removed {
            self.entries.remove(id);
        }
    }

    /// Fills `out` with the retention roster: every view *recorded*, unlike
    /// `frame_targets`, which culls unchanged views. Sorted so the backend can search it.
    pub(crate) fn collect_live_targets(&self, out: &mut Vec<TextureId>) {
        out.clear();
        out.reserve_exact(self.entries.len());
        out.extend(self.entries.values().map(|view| view.texture_id));
        out.sort_unstable();
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::identity::widget_id::{WidgetId, WidgetIdSet};
    use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
    use crate::renderer::gpu_paint::gpu_views::{GpuViews, RecordedView, ViewContent};

    fn view(id: WidgetId, paint: GpuPaintRef, content: ViewContent) -> RecordedView {
        RecordedView { id, paint, content }
    }

    /// One callback keeps one target; a different one takes its own, as `GpuPaint::init` runs once per target.
    #[test]
    fn a_replaced_callback_takes_a_fresh_target() {
        let id = WidgetId::from_hash("view");
        let mut views = GpuViews::default();
        let first = GpuPaintRef::noop();

        views.record(view(id, first.clone(), ViewContent::Changed), 1);
        let target = views.view(id).texture_id;

        let epoch = views.record(view(id, first, ViewContent::Unchanged), 2);
        assert_eq!(
            views.view(id).texture_id,
            target,
            "one callback, one target"
        );
        assert_eq!(epoch, 1, "and no repaint it did not ask for");

        let epoch = views.record(view(id, GpuPaintRef::noop(), ViewContent::Unchanged), 3);
        assert_ne!(
            views.view(id).texture_id,
            target,
            "a new callback must not inherit a target init already ran against",
        );
        assert_eq!(epoch, 3, "and it paints on the frame it arrived");
    }

    #[test]
    fn sweeping_a_removed_view_keeps_its_sibling() {
        let (gone, kept) = (WidgetId::from_hash("gone"), WidgetId::from_hash("kept"));
        let mut views = GpuViews::default();
        views.record(view(gone, GpuPaintRef::noop(), ViewContent::Changed), 1);
        views.record(view(kept, GpuPaintRef::noop(), ViewContent::Changed), 1);
        let target = views.view(kept).texture_id;

        let removed: WidgetIdSet = [gone].into_iter().collect();
        views.sweep_removed(&removed);
        let mut live = Vec::new();
        views.collect_live_targets(&mut live);
        assert_eq!(live, vec![target]);
    }
}
