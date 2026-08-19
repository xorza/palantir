//! Per-frame damage detection. Computed in [`Ui::frame`](crate::Ui::frame) after
//! `compute_rollups`; rebuilds the prev-frame snapshot in the same
//! pass via the `entry()` API — vacant slots get inserted, occupied
//! slots get diffed and either updated or evicted.
//!
//! A node is **dirty** if its `(authoring-hash, cascade-input)` differs
//! from the entry keyed by the same `WidgetId` in `DamageEngine.prev`,
//! OR it had no entry (added). A `WidgetId` present in
//! `DamageEngine.prev` with no matching node this frame contributes its
//! prev rect (removed).
//! Each contribution is folded into a [`region::DamageRegion`] via
//! its merge policy; the result drives the encoder filter and the
//! per-pass scissor list in the backend.
//!
//! **Row invariant.** `DamageEngine.prev` only holds entries for
//! widgets with at least one paint row on their last recorded frame —
//! chrome, a direct shape, or a child marker (i.e.
//! `cascade.layers[li].paint_arena.node_spans[i].len > 0`). Rowless
//! nodes (childless, chromeless, shapeless) contribute zero pixels and
//! are skipped on insert; child markers carry zero rects, so a parent
//! that paints nothing itself still can't trip the full-repaint
//! coverage threshold on add or remove. A rows→rowless transition
//! evicts the entry in the same diff loop; the prev rects contribute
//! (clear those pixels), the curr rect doesn't.
//!
//! The Vacant arm additionally skips *childless* nodes whose rows are
//! entirely off-surface — a zoomed-out canvas must not populate the map
//! with thousands of never-visible snapshots. That skip is repaid in
//! the moved-subtree arm (tier 1.5): the frame a move puts such a
//! node's rows on-surface, its snapshot is inserted there, restoring
//! the induction the prev-extent fold and the removed-widget eviction
//! tail rely on — every node painting *visible* pixels has an entry.
//!
//! **Paint order.** Child markers put the shape/child interleave into
//! each node's row span, and `compute_rollups` folds child identity
//! into `node_hash` — so a pure z-order change (raising a node, a
//! shape crossing a child boundary, two coincident shapes swapping)
//! routes its parent to the changed-paints arm, where the row
//! matcher's position map feeds the order-inversion check and each
//! inverted pair's extent overlap is damaged. Cross-parent moves are
//! the one ordering change no row span or hash captures — a widget
//! reparented (or moved between layers) at an identical rect keeps
//! every hash — so each snapshot also carries
//! [`NodeSnapshot::parent_key`], and a mismatch damages the moved
//! subtree's painted extent.
//!
//! `DamageEngine.dirty` is the per-node dirty list (added / hash- or
//! cascade-changed / evicted) in pre-order paint order. It's
//! gated behind `cfg(any(test, feature = "internals"))` — production
//! builds skip the per-node `Vec::push` entirely; tests and benches
//! assert on it through this gate.

use crate::common::block_arena::BlockArena;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::widget_id::WidgetIdMap;
use crate::scene::cascade::Cascade;
use crate::scene::cascade::paint::Paint;
use crate::scene::cascade::paint::PaintRows;
use crate::scene::damage::counters::DamageCounters;
use crate::scene::damage::node_snapshot::NodeSnapshot;
use crate::scene::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use crate::scene::damage::row_matcher::RowMatcher;
use crate::scene::damage::walk::{LayerWalk, ParentFrame};
use crate::scene::forest::Forest;
use rustc_hash::FxHashSet;
use std::time::Duration;

#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod counters;
pub(crate) mod node_snapshot;
pub(crate) mod region;
pub(crate) mod row_matcher;
mod walk;

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
#[derive(Debug)]
pub(crate) struct DamageEngine {
    /// Per-pass merge budget (extra-overdraw px) used when
    /// `compute` builds the next frame's region. Defaults to
    /// [`DEFAULT_PASS_BUDGET_PX`]; override in place (e.g. from a
    /// debug-overlay slider, a TBDR backend init, or a test) before
    /// the next `FrameCycle::post_record` runs.
    pub(crate) budget_px: f32,
    /// Last frame's snapshot, **only for widgets with paint rows last
    /// frame** (see the row invariant in the module doc).
    /// Read by the diff in `compute`, then updated/inserted/evicted
    /// in place per node. Cross-layer uniqueness of `WidgetId` is
    /// already enforced by `SeenIds::record` at recording time, so
    /// the bare `WidgetId` key is safe.
    pub(crate) prev: WidgetIdMap<NodeSnapshot>,
    /// Per-paint backing storage every `NodeSnapshot.paint_span` points
    /// into. See [`NodeSnapshot`] for the block lifecycle.
    pub(crate) paints: BlockArena<Paint>,
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
    /// [`build_row_extents`](walk::LayerWalk::build_row_extents) — the
    /// per-row screen extents (child markers swapped for their subtree's
    /// painted extent) fed to
    /// [`emit_inverted_overlaps`](walk::LayerWalk::emit_inverted_overlaps).
    /// Only filled on the rare frame a node's row order actually inverted;
    /// capacity persists so that frame allocates nothing.
    order_extents: Vec<Rect>,
    /// Retained scratch for the diff walk's parent tracking: one frame
    /// per open ancestor, `(subtree_end, WidgetId bits)`. Feeds each
    /// snapshot's [`NodeSnapshot::parent_key`].
    parent_stack: Vec<ParentFrame>,

    /// Test/bench observability for this pass — see [`DamageCounters`].
    pub(crate) counters: DamageCounters,
}

impl Default for DamageEngine {
    fn default() -> Self {
        Self {
            counters: DamageCounters::default(),
            budget_px: DEFAULT_PASS_BUDGET_PX,
            prev: WidgetIdMap::default(),
            paints: BlockArena::default(),
            matcher: RowMatcher::default(),
            raw_rects: Vec::new(),
            order_extents: Vec::new(),
            parent_stack: Vec::new(),
        }
    }
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
    pub(crate) prev_time: Option<Duration>,
    pub(crate) now: Duration,
}

/// Coverage fraction above which [`Damage::new`] stops tracking partial damage
/// and collapses straight to [`Damage::Full`]: once this much of the surface has
/// changed, the per-node filter + per-pass scissor + `LoadOp::Load` + backbuffer
/// copy bookkeeping costs more than just clearing and redrawing everything.
/// Checked against the region's sealed [`DamageRegion::coverage`]. (The
/// renderer's `DirectAdaptive` strategy applies its own, lower promote threshold
/// to the *Partial* range below this line — `DIRECT_PROMOTE_COVERAGE` in
/// `window_driver` — but that's a present-path GPU-cost call kept out of this
/// damage-tracking one.)
///
/// The threshold can sit this high because the region keeps disjoint rects
/// disjoint at the data-structure level, so `coverage` is the *sum* of
/// per-rect areas rather than the area of one bounding union. Two unrelated
/// tiny corners therefore score near 0 %, not the ~100 % a single-union
/// accumulator would report — which is what a threshold this permissive
/// depends on.
pub(crate) const FULL_REPAINT_THRESHOLD: f32 = 0.7;

/// What the GPU should do with this frame:
/// - `Skip` — nothing changed; the backbuffer is correct as-is.
/// - `Full` — clear + paint everything.
/// - `Partial(region)` — load + scissor; one render pass per rect.
///
/// Knows nothing about clear colour — that's a presentation concern
/// stamped in by [`crate::renderer::render_plan::RenderPlan`] when the
/// damage outcome is lifted into a host-facing report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Damage {
    Skip,
    Full,
    /// **Invariant:** the wrapped region is non-empty. [`Damage::new`]
    /// is the only constructor and returns [`Damage::Skip`] when the
    /// region is empty, so consumers can iterate `region.iter_rects()`
    /// without checking `is_empty` first.
    Partial(DamageRegion),
}

impl Damage {
    /// Classify a region (already sealed against its surface by
    /// [`DamageRegion::collapse_from`]) into the frame's paint decision. Pure
    /// dispatch on the precomputed `coverage` — no surface needed here; the
    /// degenerate-surface check lives at the seal site.
    ///
    /// [`DamageRegion::collapse_from`]: crate::scene::damage::region::DamageRegion::collapse_from
    pub(crate) fn new(region: DamageRegion) -> Damage {
        if region.rects.is_empty() {
            return Damage::Skip;
        }
        if region.coverage > FULL_REPAINT_THRESHOLD {
            return Damage::Full;
        }
        Damage::Partial(region)
    }
}

impl DamageEngine {
    /// Drop the per-widget previous-frame snapshot map. Called by
    /// [`Self::compute`] at entry when the caller passes
    /// `force_full = true` (surface changed, previous frame wasn't
    /// acked, or first frame) — the diff then repopulates the map
    /// from scratch but still returns `Damage::Full`.
    fn invalidate_prev(&mut self) {
        self.prev.clear();
        self.paints.clear();
    }

    /// Diff against the just-finished frame and return a
    /// [`Damage`] ready for the renderer:
    ///
    /// - [`Damage::Skip`] — empty region, nothing changed.
    /// - [`Damage::Partial`] — coverage below
    ///   [`FULL_REPAINT_THRESHOLD`].
    /// - [`Damage::Full`] — coverage above the threshold, or the
    ///   caller-supplied `force_full` (first frame / surface change /
    ///   last frame unacked), which returns early below.
    ///
    /// `self.prev` is rolled forward in the same pass via the
    /// `entry()` API: vacant slot with a painting node inserts; an
    /// occupied slot whose snapshot is unchanged is a no-op; an
    /// occupied slot whose node still paints but changed updates;
    /// an occupied slot whose node stopped painting is evicted.
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
    #[profiling::function]
    pub(crate) fn compute(
        &mut self,
        input: DamageInput<'_>,
        removed: &FxHashSet<WidgetId>,
        force_full: bool,
    ) -> Damage {
        let DamageInput {
            forest,
            cascade,
            surface,
            prev_time,
            now,
        } = input;
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
                parents: &mut self.parent_stack,
                probe: &mut self.counters,
                surface,
                force_full,
                layer,
                tree,
                cascade: &cascade.layers[layer],
            }
            .run();
        }

        // Structural diff has populated `self.prev` for next frame's
        // baseline; on `force_full` everything downstream just builds
        // a region we'd discard, so short-circuit here. The removed
        // eviction tail is a no-op in this branch (`self.prev` was
        // cleared at entry, so no stale entries survive), and the anim
        // iterator is lazy — dropping it without consuming is free.
        if force_full {
            return Damage::Full;
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
        self.finish_region(surface)
    }

    /// Pass 2: collapse the accumulated `raw_rects` into a budgeted
    /// region and lift it to a [`Damage`]. Shared tail of both compute
    /// paths.
    fn finish_region(&self, surface: Rect) -> Damage {
        let region = DamageRegion::collapse_from(&self.raw_rects, self.budget_px, surface);
        Damage::new(region)
    }

    /// PaintOnly fast path. The tree wasn't rebuilt this frame, so
    /// every node would match its prev snapshot and contribute nothing
    /// to the structural diff — skip Pass 1 entirely. Only the
    /// caller-supplied predamaged anim rects matter.
    pub(crate) fn compute_paint_only(&mut self, input: DamageInput<'_>) -> Damage {
        self.counters.begin_pass();
        self.raw_rects.clear();
        extend_predamaged(
            &mut self.raw_rects,
            input.forest,
            input.cascade,
            input.prev_time,
            input.now,
        );
        self.finish_region(input.surface)
    }
}

/// Push one screen rect into the raw-rect buffer, dropping paint-empty
/// rects — child markers (always zero) and fully clipped-away shapes
/// produce no pixels, so they have nothing to clear or repaint. Lives
/// beside `DamageEngine::raw_rects`, the buffer every caller is filling.
#[inline]
pub(super) fn push_screen(out: &mut Vec<Rect>, screen: Rect) {
    if !screen.is_paint_empty() {
        out.push(screen);
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
            let node_span = node_spans[e.node_idx as usize];
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

/// In-tree-test-only reach-in. Lives in a plain `#[cfg(test)]` impl
/// (not the feature-gated `internals` mod) because only the
/// crate's own unit tests call it — so it needs no `allow(dead_code)`
/// for the feature-only build.
#[cfg(test)]
impl DamageEngine {
    /// Union of the paint screens retained for `wid` last frame — the
    /// node's own paint extent, equal to what the live cascade's rows
    /// fold to through [`PaintRows::union_screens`]. `None` when `wid`
    /// didn't paint last frame (no `prev` entry).
    pub(crate) fn prev_paint_rect(&self, wid: WidgetId) -> Option<Rect> {
        let snap = self.prev.get(&wid)?;
        self.paints.slots[snap.paint_span.range()].union_screens()
    }
}

#[cfg(test)]
mod tests;
