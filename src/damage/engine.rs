//! The damage pass: the cross-frame snapshots it diffs against, and the walk that turns this frame's changes into a region.

use crate::cascade::Cascade;
use crate::cascade::paint::Paint;
use crate::cascade::paint::PaintRows;
use crate::common::block_arena::BlockArena;
use crate::common::tracy;
use crate::damage::Damage;
use crate::damage::counters::DamageCounters;
use crate::damage::frame_baseline::FrameBaseline;
use crate::damage::inverted_overlaps::InvertedOverlaps;
use crate::damage::node_snapshot::NodeSnapshot;
use crate::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use crate::damage::root_order::RootOrder;
use crate::damage::row_matcher::RowMatcher;
use crate::damage::walk::LayerWalk;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::{WidgetIdMap, WidgetIdSet};
use crate::scene::forest::Forest;
use std::time::Duration;

/// Output of one frame's damage pass plus the cross-frame state it reads; `prev` is mutated in place so steady-state frames don't allocate. [`Damage`] / [`DamageRegion`] are `Copy`.
#[derive(Debug, Default)]
pub(crate) struct DamageEngine {
    /// Last frame's snapshot, **only for widgets with paint rows** (see the module doc). `WidgetId` is unique across layers (`SeenIds::resolve`), so the bare key is safe.
    pub(crate) prev: WidgetIdMap<NodeSnapshot>,
    pub(crate) paints: BlockArena<Paint>,
    /// What the last presented frame was painted under, or `None`: the frame-wide half of the baseline [`Self::prev`] holds per widget. See [`FrameBaseline`], [`Self::note_presented`].
    presented: Option<FrameBaseline>,
    matcher: RowMatcher,
    /// Pass-1 scratch: every damage source appends here unmerged; pass 2 collapses it via `DamageRegion::collapse_from`.
    pub(crate) raw_rects: Vec<Rect>,

    /// Scratch for [`build_row_extents`](crate::damage::walk::LayerWalk::build_row_extents), fed to [`emit_inverted_overlaps`](crate::damage::walk::LayerWalk::emit_inverted_overlaps); filled only when a node's row order inverted.
    order_extents: Vec<Rect>,

    inversions: InvertedOverlaps,

    root_order: RootOrder,

    pub(crate) counters: DamageCounters,
}

/// Per-frame inputs shared by [`DamageEngine::compute`] and [`DamageEngine::compute_paint_only`]; `time.prev` is `None` on the first frame, when both skip predamage.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DamageInput<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) cascade: &'a Cascade,
    /// The surface rect for this frame. Zero area is a caller logic error (hosts clamp to >= 1 px), asserted in `DamageRegion::collapse_from`.
    pub(crate) surface: Rect,
    /// The frame-wide paint inputs this frame would present under; a change since the last presented frame escalates to [`Damage::Full`], as no per-node diff can see one.
    pub(crate) baseline: FrameBaseline,
    pub(crate) prev_time: Option<Duration>,
    pub(crate) now: Duration,
}

impl DamageEngine {
    /// Drops the per-widget snapshot map; [`Self::compute`] calls it on `force_full`.
    fn invalidate_prev(&mut self) {
        self.prev.clear();
        self.paints.clear();
    }

    /// Diffs against the just-finished frame and returns a [`Damage`]:
    ///
    /// - `None`: nothing changed.
    /// - [`Damage::Partial`]: coverage below [`FULL_REPAINT_THRESHOLD`](crate::damage::FULL_REPAINT_THRESHOLD).
    /// - [`Damage::Full`]: coverage above it, `force_full` (first frame, surface change, last frame unacked), or a moved [`FrameBaseline`].
    ///
    /// `self.prev` is rolled forward in the same pass; entries in `removed` are dropped afterwards. Rects are in **screen space** (`Paint.screen`), matching where the GPU paints. See [`DamageInput::surface`].
    pub(crate) fn compute(
        &mut self,
        input: DamageInput<'_>,
        removed: &WidgetIdSet,
        force_full: bool,
    ) -> Option<Damage> {
        tracy::zone!();
        let DamageInput {
            forest,
            cascade,
            surface,
            baseline,
            prev_time,
            now,
        } = input;
        // Frame-wide inputs join the `force_full` signal: a moved baseline repaints the whole surface, and no snapshot carries it.
        let force_full = force_full || self.presented != Some(baseline);
        // `force_full` means a fresh frame. Dropping the snapshot map here keeps a caller from passing it without the invalidation; the diff still repopulates the map and `Damage::Full` is returned.
        if force_full {
            self.invalidate_prev();
        }
        self.counters.begin_pass();

        // Pass 1: every source (structural diff, order inversions, predamaged anim rects, removed eviction tail) pushes into `raw_rects` unmerged.
        self.raw_rects.clear();

        for (layer, tree) in forest.trees.iter_paint_order() {
            LayerWalk {
                prev: &mut self.prev,
                paints: &mut self.paints,
                matcher: &mut self.matcher,
                raw_rects: &mut self.raw_rects,
                order_extents: &mut self.order_extents,
                inversions: &mut self.inversions,
                counters: &mut self.counters,
                surface,
                force_full,
                layer,
                tree,
                cascade: &cascade.layers[layer],
            }
            .run();
            self.root_order.diff(
                layer,
                tree,
                &cascade.layers[layer],
                force_full,
                &mut self.raw_rects,
            );
        }

        // `self.prev` is populated for next frame; on `force_full` the rest would build a discarded region, so short-circuit.
        if force_full {
            return self.note_presented(baseline, Some(Damage::Full));
        }

        // Predamaged anim rects: the structural diff is content-only and misses phase flips (bumping `node_hash` would invalidate MeasureCache for no layout change); the encoder's `PaintAnimCursor::sample` decides per rect.
        extend_predamaged(&mut self.raw_rects, forest, cascade, prev_time, now);

        // Removed-widget eviction tail: every remaining `prev` entry painted last frame. Pushed decomposed (chrome plus per-shape) so a multi-shape owner doesn't push the gaps.
        for wid in removed {
            if let Some(snap) = self.prev.remove(wid) {
                self.raw_rects
                    .extend(self.paints.slots[snap.paint_span.range()].screens());
                self.paints.release(snap.paint_span);
            }
        }

        let damage = self.finish_region(surface);
        self.note_presented(baseline, damage)
    }

    /// Stamps the baseline a frame presents under and hands its damage back. Only a painted frame presents; stamping a skipped one would adopt a clear colour no pixel was painted under.
    const fn note_presented(
        &mut self,
        baseline: FrameBaseline,
        damage: Option<Damage>,
    ) -> Option<Damage> {
        if damage.is_some() {
            self.presented = Some(baseline);
        }
        damage
    }

    fn finish_region(&self, surface: Rect) -> Option<Damage> {
        Damage::new(DamageRegion::collapse_from(
            &self.raw_rects,
            DEFAULT_PASS_BUDGET_PX,
            surface,
        ))
    }

    /// PaintOnly fast path: the tree wasn't rebuilt, so Pass 1 is skipped and only predamaged anim rects remain, unless a frame-wide input moved.
    pub(crate) fn compute_paint_only(&mut self, input: DamageInput<'_>) -> Option<Damage> {
        tracy::zone!();
        self.counters.begin_pass();
        // A theme swapped between frames of a retained tree reaches the surface here or nowhere. `prev` stands: it describes the tree being repainted.
        if self.presented != Some(input.baseline) {
            return self.note_presented(input.baseline, Some(Damage::Full));
        }
        self.raw_rects.clear();
        extend_predamaged(
            &mut self.raw_rects,
            input.forest,
            input.cascade,
            input.prev_time,
            input.now,
        );
        let damage = self.finish_region(input.surface);
        self.note_presented(input.baseline, damage)
    }
}

fn extend_predamaged(
    out: &mut Vec<Rect>,
    forest: &Forest,
    cascade: &Cascade,
    prev_time: Option<Duration>,
    now: Duration,
) {
    // No prev frame: Pass 1 already contributed every painting widget, and an anim rect is a sub-rect of its owner.
    let Some(prev) = prev_time else { return };
    for (layer, tree) in forest.trees.iter_paint_order() {
        let arena = &cascade.layers[layer].paint_arena;
        let paints = &arena.rows;
        let node_spans = &arena.node_spans;
        for e in &tree.paint_anims.entries {
            if e.anim.next_wake(prev).is_none_or(|wake| wake > now) {
                continue;
            }
            let node_span = node_spans[e.node.idx()];
            // `e.row` came from the recording counter and `compute_paint_rect` emits rows in the same order, so a miss means they drifted.
            debug_assert!(
                e.row < node_span.len,
                "paint-anim row {} out of the owner's {} paint rows",
                e.row,
                node_span.len,
            );
            out.push(paints[(node_span.start + e.row) as usize].screen);
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::cascade::paint::Paint;
    use crate::cascade::paint::PaintRows as _;
    use crate::damage::engine::DamageEngine;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::widget_id::WidgetId;

    impl DamageEngine {
        /// Union of the paint screens retained for `wid` last frame; `None` if it didn't paint, [`Rect::ZERO`] if its rows painted nothing.
        pub(crate) fn prev_paint_rect(&self, wid: WidgetId) -> Option<Rect> {
            let snap = self.prev.get(&wid)?;
            Some(self.paints.slots[snap.paint_span.range()].union_screens())
        }

        /// The paint rows retained for `wid` last frame, in row order: chrome first, then direct shapes.
        pub(crate) fn prev_paint_rows(&self, wid: WidgetId) -> &[Paint] {
            let snap = self.prev.get(&wid).expect("the widget painted last frame");
            &self.paints.slots[snap.paint_span.range()]
        }
    }
}
