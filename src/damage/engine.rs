//! The damage pass: the cross-frame snapshots it diffs against, and
//! the walk that turns this frame's changes into a region.

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

/// Output of one frame's damage pass plus the cross-frame state it
/// reads to produce that output.
///
/// `prev` is the per-`WidgetId` snapshot map carried over from last
/// frame; it's mutated in place during `compute` (read old, write
/// new) so steady-state frames don't allocate. `paints` holds the
/// per-paint backing storage those snapshots span — see
/// [`NodeSnapshot`].
///
/// Capacities on `prev` are retained across frames; the returned
/// [`Damage`] / [`DamageRegion`] is `Copy` and threads through
/// `FrameOutput` by value.
#[derive(Debug, Default)]
pub(crate) struct DamageEngine {
    /// Last frame's snapshot, **only for widgets with paint rows last
    /// frame** (see the row invariant in the module doc).
    /// Read by the diff in `compute`, then updated/inserted/evicted
    /// in place per node. Cross-layer uniqueness of `WidgetId` is
    /// already enforced by `SeenIds::resolve` at recording time, so
    /// the bare `WidgetId` key is safe.
    pub(crate) prev: WidgetIdMap<NodeSnapshot>,
    /// Per-paint backing storage every `NodeSnapshot.paint_span` points
    /// into. See [`NodeSnapshot`] for the block lifecycle.
    pub(crate) paints: BlockArena<Paint>,
    /// What the last presented frame was painted under, or `None` before
    /// one was. The frame-wide half of the baseline [`Self::prev`] holds
    /// the per-widget half of — see [`FrameBaseline`] for why a diff of
    /// snapshots cannot see these move, and
    /// [`Self::note_presented`] for when this is stamped.
    presented: Option<FrameBaseline>,
    /// Retained scratch for the per-node row pairing. Beside the storage
    /// rather than wrapped with it: the diff slices live spans out of
    /// `paints.slots` on every leg, so a wrapper that owned both only hid
    /// where the storage was.
    matcher: RowMatcher,
    /// Pass-1 scratch buffer. `compute` walks every damage source
    /// (structural diff, predamaged anim rects, removed-widget evict)
    /// and appends each contribution here without applying the merge
    /// policy. Pass 2 hands this slice to `DamageRegion::collapse_from`
    /// which produces the bounded region. Retained capacity — no
    /// per-frame allocation in steady state.
    pub(crate) raw_rects: Vec<Rect>,

    /// Retained scratch for
    /// [`build_row_extents`](crate::damage::walk::LayerWalk::build_row_extents) — the
    /// per-row screen extents (child markers swapped for their subtree's
    /// painted extent) fed to
    /// [`emit_inverted_overlaps`](crate::damage::walk::LayerWalk::emit_inverted_overlaps).
    /// Only filled on the rare frame a node's row order actually inverted;
    /// capacity persists so that frame allocates nothing.
    order_extents: Vec<Rect>,

    /// Retained scratch for the inversion damage of a node's rows.
    inversions: InvertedOverlaps,

    /// Each layer's root order, which no snapshot holds — see
    /// [`RootOrder`].
    root_order: RootOrder,

    /// Test/bench observability for this pass — see [`DamageCounters`].
    pub(crate) counters: DamageCounters,
}

/// Per-frame inputs shared by [`DamageEngine::compute`] and
/// [`DamageEngine::compute_paint_only`]. The fields that differ
/// between the two entry points (`removed`, `force_full`) stay as
/// dedicated args on `compute` — passing them through this struct
/// would force `compute_paint_only` to fabricate dummies.
///
/// `time.prev` is `None` on the first frame (no prior `now` to anim
/// against); both compute paths short-circuit predamage in that case.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DamageInput<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) cascade: &'a Cascade,
    /// WindowDriver-arranged surface rect for this frame. A degenerate
    /// zero-area surface is a caller logic error: hosts clamp physical
    /// size to ≥ 1 px and skip occluded windows before `Ui::frame`
    /// runs, and `DamageRegion::collapse_from` asserts on it — the one
    /// site that divides by surface area — rather than degrading
    /// silently.
    pub(crate) surface: Rect,
    /// The frame-wide paint inputs this frame would present under. A
    /// change since the last presented frame escalates to
    /// [`Damage::Full`], because no per-node diff can see one.
    pub(crate) baseline: FrameBaseline,
    pub(crate) prev_time: Option<Duration>,
    pub(crate) now: Duration,
}

impl DamageEngine {
    /// Drop the per-widget previous-frame snapshot map. Called by
    /// [`Self::compute`] at entry when the caller passes
    /// `force_full = true` (surface changed, previous frame wasn't
    /// acked, or first frame) — the diff then repopulates the map
    /// from scratch but still returns `Some(Damage::Full)`.
    fn invalidate_prev(&mut self) {
        self.prev.clear();
        self.paints.clear();
    }

    /// Diff against the just-finished frame and return a
    /// [`Damage`] ready for the renderer:
    ///
    /// - `None` — empty region, nothing changed.
    /// - [`Damage::Partial`] — coverage below
    ///   [`FULL_REPAINT_THRESHOLD`](crate::damage::FULL_REPAINT_THRESHOLD).
    /// - [`Damage::Full`] — coverage above the threshold, the
    ///   caller-supplied `force_full` (first frame / surface change /
    ///   last frame unacked), or a [`FrameBaseline`] that moved since
    ///   the last presented frame. All three return early below.
    ///
    /// `self.prev` is rolled forward in the same pass: a missing entry
    /// with a painting node inserts; an unchanged snapshot is a no-op;
    /// a node that still paints but changed updates; a node that stopped
    /// painting is removed. `get` / `insert` / `remove` rather than
    /// `entry`, because the classification reads before it knows whether
    /// it will write at all, and the no-op arm is the common one.
    /// Last-frame entries listed in `removed` (precomputed by
    /// [`crate::scene::seen_ids::SeenIds`] so damage and `text` reuse
    /// the diff) are dropped afterwards.
    ///
    /// Rects are tracked in **screen space** (the per-shape
    /// `Paint.screen` rects — each the transformed shape bbox inflated
    /// by ink overhang, then ancestor-clipped — and their union). This
    /// makes damage match where the GPU actually paints, so the backend
    /// scissor lands on the right pixels even under transformed
    /// parents or around a drop shadow.
    ///
    /// `surface` is the rect the host arranged the UI into this
    /// frame; see [`DamageInput::surface`] for the degenerate-surface
    /// behavior.
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
        // The frame-wide inputs join the caller's signal rather than
        // getting a flag of their own: a baseline that moved repaints
        // the whole surface, which is what the one flag already means,
        // and the map rebuild it brings costs a fraction of the repaint
        // it accompanies. What no snapshot carries, no diff can find —
        // so this is where a moved one is caught.
        let force_full = force_full || self.presented != Some(baseline);
        // `force_full` is the "treat as a fresh frame" signal — set
        // by the caller when `FrameRuntime::take_frame_plan` decided
        // this frame must repaint everything (surface changed, last
        // frame wasn't acked, or first frame). Drop the per-widget
        // snapshot map here — owning the pairing keeps a caller from
        // passing `force_full` without the invalidation and corrupting
        // the next incremental diff with stale spans — then run the
        // full diff pass to repopulate it for next frame, just return
        // `Damage::Full` instead of the filtered region.
        if force_full {
            self.invalidate_prev();
        }
        self.counters.begin_pass();

        // Pass 1: every damage source pushes its contributions into
        // `self.raw_rects` without applying the merge or budget
        // policy. Sources: structural diff (added / hash-changed /
        // removed widget), paint-order inversions, predamaged anim
        // rects, and the `removed`-set eviction tail. Pass 2 collapses
        // the buffer into the bounded region.
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

        // Structural diff has populated `self.prev` for next frame's
        // baseline; on `force_full` everything downstream just builds
        // a region we'd discard, so short-circuit here. The removed
        // eviction tail is a no-op in this branch (`self.prev` was
        // cleared at entry, so no stale entries survive), and the anim
        // iterator is lazy — dropping it without consuming is free.
        if force_full {
            return self.note_presented(baseline, Some(Damage::Full));
        }

        // Predamaged anim rects. The structural diff above is
        // content-only and (intentionally) doesn't pick up phase
        // flips — bumping `node_hash` / `subtree_hash` would
        // invalidate MeasureCache for the owner's ancestor chain on
        // every flip even though layout didn't change. The encoder's
        // `PaintAnimCursor::sample` decides per-rect whether to emit a
        // quad (visible half) or skip (hidden half).
        extend_predamaged(&mut self.raw_rects, forest, cascade, prev_time, now);

        // Removed-widget eviction tail. Every remaining `prev` entry
        // painted last frame (invariant), so its parts always
        // contribute. Push decomposed — chrome + per-shape — so a
        // multi-shape owner going away pushes its actual painted
        // footprint, not the union of disjoint shapes plus the gaps
        // between them.
        for wid in removed {
            if let Some(snap) = self.prev.remove(wid) {
                self.raw_rects
                    .extend(self.paints.slots[snap.paint_span.range()].screens());
                self.paints.release(snap.paint_span);
            }
        }

        // Pass 2: collapse to the bounded region.
        let damage = self.finish_region(surface);
        self.note_presented(baseline, damage)
    }

    /// Stamp the baseline a frame presents under, and hand its damage
    /// back.
    ///
    /// Only a frame that paints presents: the screen still holds
    /// whatever the last painted frame put there, so a skipped frame
    /// leaves that frame's baseline standing. Stamping regardless would
    /// adopt a clear colour no pixel was ever painted under, and the
    /// change would then never reach the screen at all.
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

    /// Pass 2: collapse the accumulated `raw_rects` into a budgeted
    /// region and lift it to a [`Damage`]. Shared tail of both compute
    /// paths.
    fn finish_region(&self, surface: Rect) -> Option<Damage> {
        Damage::new(DamageRegion::collapse_from(
            &self.raw_rects,
            DEFAULT_PASS_BUDGET_PX,
            surface,
        ))
    }

    /// PaintOnly fast path. The tree wasn't rebuilt this frame, so
    /// every node would match its prev snapshot and contribute nothing
    /// to the structural diff — skip Pass 1 entirely. The predamaged
    /// anim rects are then all a frame of this kind can have, unless a
    /// frame-wide input moved.
    pub(crate) fn compute_paint_only(&mut self, input: DamageInput<'_>) -> Option<Damage> {
        tracy::zone!();
        self.counters.begin_pass();
        // The tree is retained, so a theme swapped between two frames of
        // it reaches the surface here or nowhere. `prev` stands: it
        // describes the very tree this frame repaints, and no walk runs
        // to rebuild it if it were dropped.
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
    // No prev frame ⇒ Pass 1 already contributed every painting
    // widget's rect (every entry was Vacant), and a paint-anim rect
    // is always a sub-rect of its owner — nothing new to add.
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
            // `e.row` was captured from the recording counter
            // (`OpenFrame::paint_rows`), and `compute_paint_rect` emits
            // one row per chrome/shape/child in the same record order,
            // so the slot must exist — a miss means the cascade emit
            // and the recording counter drifted apart.
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
        /// Union of the paint screens retained for `wid` last frame — the
        /// node's own paint extent, equal to what the live cascade's rows
        /// fold to through [`PaintRows::union_screens`]. `None` when `wid`
        /// didn't paint last frame (no `prev` entry); [`Rect::ZERO`] when it
        /// had rows that painted nothing.
        pub(crate) fn prev_paint_rect(&self, wid: WidgetId) -> Option<Rect> {
            let snap = self.prev.get(&wid)?;
            Some(self.paints.slots[snap.paint_span.range()].union_screens())
        }

        /// The paint rows retained for `wid` last frame, in row order:
        /// chrome first when the node has any, then its direct shapes.
        pub(crate) fn prev_paint_rows(&self, wid: WidgetId) -> &[Paint] {
            let snap = self.prev.get(&wid).expect("the widget painted last frame");
            &self.paints.slots[snap.paint_span.range()]
        }
    }
}
