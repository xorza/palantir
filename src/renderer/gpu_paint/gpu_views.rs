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
    /// texture does not exist yet — and is the one place an id is minted,
    /// which is what makes the `TextureId` stable for the view's whole
    /// life.
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
                entry.paint = paint;
                if repaint {
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
