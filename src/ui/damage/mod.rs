//! Per-frame damage detection. Computed in [`Ui::frame`] after
//! `compute_hashes`; rebuilds the prev-frame snapshot in the same
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
//! **Painting-only invariant.** `DamageEngine.prev` only holds entries for
//! widgets that painted on their last recorded frame (have chrome OR
//! direct shapes — i.e. `cascades.layers[li].paint_arena.node_spans[i].len > 0`).
//! Non-painting nodes contribute zero pixels, so they're skipped on
//! insert. A painting→non-painting transition evicts the entry in the
//! same diff loop; the prev rects contribute (clear those pixels), the
//! curr rect doesn't.
//!
//! `DamageEngine.dirty` is the per-node dirty list (added / hash- or
//! cascade-changed / evicted) in pre-order paint order. It's
//! gated behind `cfg(any(test, feature = "internals"))` — production
//! builds skip the per-node `Vec::push` entirely; tests and benches
//! assert on it through this gate.

use crate::forest::Forest;
use crate::forest::rollups::{CascadeInputHash, NodeHash};
use crate::forest::seen_ids::WidgetIdMap;
use crate::forest::tree::NodeId;
use crate::primitives::approx::EPS;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::{Cascades, Paint};
use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use rustc_hash::FxHashSet;
use std::collections::hash_map::Entry;
use std::time::Duration;

pub mod region;

/// Per-painting-widget snapshot held in [`DamageEngine::prev`], keyed by
/// stable [`WidgetId`]. Only widgets that painted last frame have an
/// entry — non-painting nodes (e.g. a popup's invisible click-eater)
/// are skipped on insert, so their full-surface rect can't trip the
/// full-repaint coverage threshold on add or remove.
///
/// **Storage shape.** Per-paint snapshots don't live inline here —
/// they live in [`DamageEngine::arena`], a single contiguous
/// arena shared by every painting widget, and this struct just holds
/// a `Span` into it. Each row is either chrome (row 0 when present)
/// or one direct shape, mirroring `Cascades::paint_arenas`.
///
/// **No cached `rect`.** The node's own paint extent (the union of its
/// `paint_arena` rows — what the cascade used to store as
/// `Cascade.paint_rect`) is a pure function of `(hash, cascade_input)`:
/// every geometry input (`layout_rect`, ancestor transform/clip) lives
/// in `cascade_input` and every shape input lives in `hash`, so a
/// snapshot field would be a redundant cache of those two. The diff
/// keys the "node unchanged" fast path on `(hash, cascade_input)`
/// directly; the per-shape screen rects needed when something *did*
/// change are recovered from `paint_span`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// Slice into [`DamageEngine::arena`] describing this
    /// widget's per-paint snapshots in record order (chrome at row 0
    /// when present, then shapes). Empty span for non-painting nodes
    /// — though the painting-only invariant means they don't get an
    /// entry in `prev` at all.
    pub(crate) paint_span: Span,
    /// Authoring hash from last frame's `Tree.rollups.node`.
    pub(crate) hash: NodeHash,
    /// Rollup hash of this node + its entire subtree from last frame's
    /// `Tree.rollups.subtree`. Pair with `cascade_input` to drive the
    /// subtree-skip fast path: if both match the current frame, every
    /// descendant is bit-identical and the per-node diff can jump to
    /// `subtree_end[i]`.
    pub(crate) subtree_hash: NodeHash,
    /// Fingerprint of last frame's cascade inputs at this node (parent
    /// transform/clip/disabled/invisible + own arranged rect). See
    /// [`crate::forest::rollups::CascadeInputHash`].
    pub(crate) cascade_input: CascadeInputHash,
}

/// Per-painting-widget paint snapshots packed contiguously, plus the
/// scratch buffer + orphan counter that drive double-buffered
/// compaction. Each [`NodeSnapshot::paint_span`] is a slice into
/// `snaps`; chrome lives at row 0 of the owner's span when present,
/// followed by direct shapes in record order.
///
/// Lifecycle: append-only on count-change paths (new span at end,
/// old slice orphaned); in-place on same-count refreshes;
/// [`Self::maybe_compact`] reseats live spans into `scratch` once
/// orphans exceed the threshold, then swaps. Retained capacity —
/// steady-state alloc-free even under paint-count churn.
#[derive(Default)]
pub(crate) struct PaintSnapArena {
    pub(crate) snaps: Vec<Paint>,
    /// Reusable destination for compaction (and a swap target). Same
    /// invariants as `snaps` after a `swap`.
    scratch: Vec<Paint>,
    /// Retained "which prev paints have been claimed?" bitmap for the
    /// content-keyed slow path in [`Self::diff_changed_leg`]. Sized to
    /// `prev_span.len` per call; capacity is reused so steady-state
    /// content reshuffles don't allocate.
    prev_matched: Vec<bool>,
    /// Curr indices that pass 1 of [`Self::diff_changed_leg`] couldn't
    /// pair on exact `(screen, hash)`. Empty after pass 1 → pass 2 is
    /// skipped entirely (the common "shapes reshuffled but content
    /// unchanged" case). Capacity retained across frames so the slow
    /// path stays alloc-free.
    pending_curr: Vec<u32>,
    /// Count of `Paint` entries in `snaps` that no live
    /// `NodeSnapshot::paint_span` points into. Drives the compaction
    /// trigger.
    orphaned: u32,
    /// Compaction-event counter — bumped each time [`Self::compact`]
    /// runs. Gated behind `internals` so benches can verify the path
    /// was actually exercised.
    #[cfg(any(test, feature = "internals"))]
    compactions_run: u32,
}

/// Output of one frame's damage pass plus the cross-frame state it
/// reads to produce that output.
///
/// `prev` is the per-`WidgetId` snapshot map carried over from last
/// frame; it's mutated in place during `compute` (read old, write
/// new) so steady-state frames don't allocate. `arena` holds the
/// per-paint backing storage for those snapshots — see
/// [`PaintSnapArena`].
///
/// Capacities on `prev` are retained across frames; the returned
/// [`Damage`] / [`DamageRegion`] is `Copy` and threads through
/// `FrameOutput` by value.
pub(crate) struct DamageEngine {
    /// Per-pass merge budget (extra-overdraw px) used when
    /// `compute` builds the next frame's region. Defaults to
    /// [`DEFAULT_PASS_BUDGET_PX`]; override in place (e.g. from a
    /// debug-overlay slider, a TBDR backend init, or a test) before
    /// the next `Ui::post_record` runs.
    pub(crate) budget_px: f32,
    /// Last frame's snapshot, **only for widgets that painted last
    /// frame** (see the painting-only invariant in the module doc).
    /// Read by the diff in `compute`, then updated/inserted/evicted
    /// in place per node. Cross-layer uniqueness of `WidgetId` is
    /// already enforced by `SeenIds::record` at recording time, so
    /// the bare `WidgetId` key is safe.
    pub(crate) prev: WidgetIdMap<NodeSnapshot>,
    /// Paint-snap arena referenced by every `NodeSnapshot.paint_span`.
    /// See [`PaintSnapArena`] for the lifecycle.
    pub(crate) arena: PaintSnapArena,
    /// Pass-1 scratch buffer. `compute` walks every damage source
    /// (structural diff, predamaged anim rects, removed-widget evict)
    /// and appends each contribution here without applying the merge
    /// policy. Pass 2 hands this slice to `DamageRegion::collapse_from`
    /// which produces the bounded region. Retained capacity — no
    /// per-frame allocation in steady state.
    pub(crate) raw_rects: Vec<Rect>,

    /// Count of subtree-skip jumps the last `compute` performed —
    /// every match of the tier-1 subtree-skip arm jumped `subtree_end - i`
    /// instead of advancing by 1. Read by tests and benches via
    /// `support::internals::damage_subtree_skips`; zero on first
    /// frame and on full-repaint fall-through. Gated alongside
    /// `dirty` — production builds don't pay the increment.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) subtree_skips: u32,
    #[cfg(any(test, feature = "internals"))]
    pub(crate) dirty: Vec<NodeId>,
}

impl Default for DamageEngine {
    fn default() -> Self {
        Self {
            #[cfg(any(test, feature = "internals"))]
            dirty: Vec::new(),
            budget_px: DEFAULT_PASS_BUDGET_PX,
            prev: WidgetIdMap::default(),
            arena: PaintSnapArena::default(),
            raw_rects: Vec::new(),
            #[cfg(any(test, feature = "internals"))]
            subtree_skips: 0,
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
#[derive(Clone, Copy)]
pub(crate) struct DamageInput<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) cascades: &'a Cascades,
    /// Host-arranged surface rect for this frame. A degenerate
    /// zero-area surface shouldn't happen in practice (host filters
    /// resize-to-zero, and a surface *change* takes the `force_full`
    /// path before the diff runs); if one slips through, every region
    /// rect surface-clips to empty so [`Damage::new`] returns
    /// [`Damage::Skip`].
    pub(crate) surface: Rect,
    pub(crate) prev_time: Option<Duration>,
    pub(crate) now: Duration,
}

/// Coverage ratio above which the renderer should skip the per-node
/// filter and clear-redraw the whole surface. Below this, the
/// bookkeeping (per-pass scissor + `LoadOp::Load` + backbuffer copy)
/// wins; above it, the savings are eaten by the overhead. The
/// previous 0.5 was tuned for the single-rect-union accumulator
/// where two unrelated tiny corners would blow the union to ~100 %
/// and trip the threshold despite < 1 % of pixels actually
/// changing. The multi-rect region keeps disjoint corners disjoint
/// at the data structure level, so the threshold is now applied to
/// the *sum* of per-rect areas — corner-pair pathologies stay well
/// below 0.7.
pub(crate) const FULL_REPAINT_THRESHOLD: f32 = 0.7;

/// Minimum [`PaintSnapArena::snaps`] length before [`PaintSnapArena::maybe_compact`]
/// considers running. Below this the arena is small enough that the
/// reseat walk costs more than the orphaned-slot memory it would
/// reclaim — capacity is `Vec`-amortised and these entries stay hot
/// in cache. Empirically tuned against `benches/damage.rs`; change
/// with a benchmark on the damage-merge fixture.
const COMPACT_MIN_TOTAL: u32 = 256;

/// Orphan-ratio threshold (in 1/4 units) above which compaction
/// triggers — `orphaned * 4 >= total * COMPACT_ORPHAN_RATIO_NUM` is
/// the predicate. `3/4 = 75%` orphaned means three quarters of the
/// arena is dead bytes before a reseat pays off; lower values cause
/// thrash on churn-heavy frames. Same TODO as `COMPACT_MIN_TOTAL`.
const COMPACT_ORPHAN_RATIO_NUM: u32 = 3;

/// What the GPU should do with this frame:
/// - `None` — nothing changed; the backbuffer is correct as-is.
/// - `Full` — clear + paint everything.
/// - `Partial(region)` — load + scissor; one render pass per rect.
///
/// Knows nothing about clear colour — that's a presentation concern
/// stamped in by [`crate::ui::frame_report::RenderPlan`] when the
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
    /// True iff this is the skip signal — caller can short-circuit
    /// the renderer entirely.
    #[inline]
    pub(crate) fn is_skip(self) -> bool {
        matches!(self, Damage::Skip)
    }

    pub(crate) fn new(surface: Rect, region: DamageRegion) -> Damage {
        if region.is_empty() {
            return Damage::Skip;
        }
        let surface_area = surface.area();
        assert!(surface_area > EPS);

        // Region rects are surface-clipped at `collapse_from` (see
        // the doc on `DamageRegion::collapse_from`), so `total_area`
        // is already the *visible* footprint — counting off-surface
        // pixels here would be wrong by definition (a paint_rect on
        // a root-level transformed canvas with no clip ancestor can
        // extend far past the viewport at high zoom). Pinned by
        // `partial_when_oversized_rect_lies_mostly_off_surface`.
        if region.total_area() / surface_area > FULL_REPAINT_THRESHOLD {
            return Damage::Full;
        }
        Damage::Partial(region)
    }
}

/// Result of [`PaintSnapArena::diff_changed_leg`].
pub(crate) struct ChangedLeg {
    /// Span covering this frame's paints — `prev_span` reused on the
    /// fast path, a fresh tail span on the slow path.
    pub(crate) span: Span,
    /// True when every `Paint` matched bit-identically (the fast path),
    /// so the per-shape diff emitted *no* damage. Reaching the
    /// changed-paints arm at all means `hash` or `cascade_input`
    /// changed, so a `true` here means a cascade-state toggle (ancestor
    /// disabled / clip-saturated pan) altered the node's pixels without
    /// moving any shape — the caller must damage the union to repaint
    /// it.
    pub(crate) geometry_unchanged: bool,
}

impl PaintSnapArena {
    /// Reset to empty — caller's next `compute` will repopulate.
    pub(crate) fn clear(&mut self) {
        self.snaps.clear();
        self.orphaned = 0;
    }

    /// Append `paints` to the tail and return the covering [`Span`].
    /// Used by the Vacant-insert arm of `compute`.
    pub(crate) fn append(&mut self, paints: &[Paint]) -> Span {
        let start = self.snaps.len() as u32;
        self.snaps.extend_from_slice(paints);
        Span::new(start, paints.len() as u32)
    }

    /// Per-paint diff leg for the changed-paints arm. Three strategies
    /// in order of cost:
    ///
    /// **Fast path** — bit-identical positional match across the whole
    /// span. Common when only ancestor state changed: the per-node hash
    /// flipped but the paints themselves carry the same `(screen, hash)`
    /// in the same order. Zero damage rects, span reused in place.
    ///
    /// **Slow path** — two-pass content-keyed match. Pass 1 pairs
    /// each curr paint with the first unclaimed prev paint of identical
    /// `(screen, hash)` (no damage — same shape, same place). Pass 2
    /// handles still-unmatched curr paints by looking for an unclaimed
    /// prev with matching `hash` only: if found, emit *both* rects as
    /// move damage; otherwise emit the curr rect alone (added or
    /// content-changed). Prev paints left unclaimed are removals.
    /// Exact-first ordering matters: it preserves the "shape stayed
    /// put" pairing even when another shape with the same `hash`
    /// moved within the same node, avoiding the spurious move-damage
    /// a single-pass matcher would emit.
    ///
    /// Sub-pixel float wobble on `Paint.screen` (composer's pixel
    /// snapping runs downstream) makes strict `==` brittle; the
    /// hash-only fallback recovers the move signal without losing the
    /// exact-match optimisation.
    ///
    /// Linear scan per curr paint is O(n·m); the retained
    /// `prev_matched` bitmap and `pending_curr` index list keep both
    /// passes alloc-free across frames. Pass 1 collects unmatched
    /// curr indices into `pending_curr`; pass 2 walks only those —
    /// `pending_curr.is_empty()` (every shape paired exactly) skips
    /// pass 2 outright. The slow path spills `curr_paints` to the
    /// tail of `snaps` and routes the prev span through
    /// [`Self::mark_orphaned`]; `maybe_compact` reclaims the tail
    /// once orphans accumulate.
    pub(crate) fn diff_changed_leg(
        &mut self,
        out: &mut Vec<Rect>,
        prev_span: Span,
        curr_paints: &[Paint],
    ) -> ChangedLeg {
        let prev_start = prev_span.start as usize;
        let prev_len = prev_span.len as usize;
        let prev_slice = &self.snaps[prev_start..prev_start + prev_len];

        if prev_len == curr_paints.len() && prev_slice.iter().zip(curr_paints).all(|(p, c)| p == c)
        {
            return ChangedLeg {
                span: prev_span,
                geometry_unchanged: true,
            };
        }

        // Split-borrow: the matching passes read prev_slice (& self.snaps)
        // and write the scratch bitmap + pending-index list simultaneously.
        let Self {
            snaps,
            prev_matched,
            pending_curr,
            ..
        } = self;
        let prev_slice = &snaps[prev_start..prev_start + prev_len];

        prev_matched.clear();
        prev_matched.resize(prev_len, false);
        pending_curr.clear();

        // Pass 1 — exact (screen, hash) pairs. No damage. Curr indices
        // that didn't pair queue up for pass 2.
        for (j, &c) in curr_paints.iter().enumerate() {
            let mut matched = false;
            for (i, &p) in prev_slice.iter().enumerate() {
                if !prev_matched[i] && p == c {
                    prev_matched[i] = true;
                    matched = true;
                    break;
                }
            }
            if !matched {
                pending_curr.push(j as u32);
            }
        }
        // Pass 2 — hash-only pairs surface as moves; unmatched curr
        // surfaces as adds. Skipped entirely when pass 1 paired every
        // curr paint (the common "reshuffled but content unchanged"
        // case).
        for &j in pending_curr.iter() {
            let c = curr_paints[j as usize];
            let mut moved = false;
            for (i, &p) in prev_slice.iter().enumerate() {
                if !prev_matched[i] && p.hash == c.hash {
                    out.push(p.screen);
                    out.push(c.screen);
                    prev_matched[i] = true;
                    moved = true;
                    break;
                }
            }
            if !moved {
                out.push(c.screen);
            }
        }
        // Remaining prev paints — removals.
        for (i, &p) in prev_slice.iter().enumerate() {
            if !prev_matched[i] {
                out.push(p.screen);
            }
        }

        let new_start = snaps.len() as u32;
        snaps.extend_from_slice(curr_paints);
        self.mark_orphaned(prev_len as u32);
        ChangedLeg {
            span: Span::new(new_start, curr_paints.len() as u32),
            geometry_unchanged: false,
        }
    }

    /// Mark `n` paint entries as orphaned (their owning snapshot was
    /// evicted or its span was relocated). Saturating to avoid wrap
    /// in the unlikely 4-billion-orphan edge case.
    #[inline]
    pub(crate) fn mark_orphaned(&mut self, n: u32) {
        self.orphaned = self.orphaned.saturating_add(n);
    }

    /// Walk live `NodeSnapshot::paint_span`s in pre-order paint
    /// order and reseat into `scratch`, then swap.
    pub(crate) fn compact(&mut self, forest: &Forest, prev: &mut WidgetIdMap<NodeSnapshot>) {
        self.scratch.clear();
        for (_layer, tree) in forest.iter_paint_order() {
            for wid in tree.records.widget_id() {
                let Some(snap) = prev.get_mut(wid) else {
                    continue;
                };
                // Painting-only invariant: every entry in `prev`
                // covers at least one Paint row (chrome at row 0 OR
                // ≥1 shape). The unified `paint_arena.rows` doesn't
                // distinguish chrome from shape spans, so a
                // chrome-only owner contributes one row, not zero.
                // A zero-len snap would have a stale `start` after
                // the swap below — assert rather than silently skip.
                assert!(
                    snap.paint_span.len > 0,
                    "PaintSnapArena::compact: prev entry for {wid:?} has zero-len paint_span, \
                     violating the painting-only invariant",
                );
                let new_start = self.scratch.len() as u32;
                self.scratch
                    .extend_from_slice(&self.snaps[snap.paint_span.range()]);
                snap.paint_span = Span::new(new_start, snap.paint_span.len);
            }
        }
        std::mem::swap(&mut self.snaps, &mut self.scratch);
        self.orphaned = 0;
        #[cfg(any(test, feature = "internals"))]
        {
            self.compactions_run = self.compactions_run.saturating_add(1);
        }
    }

    /// Trigger compaction when the arena is large enough
    /// ([`COMPACT_MIN_TOTAL`]) and orphaned entries are ≥ 75 % of the
    /// buffer ([`COMPACT_ORPHAN_RATIO_NUM`]/4).
    pub(crate) fn maybe_compact(&mut self, forest: &Forest, prev: &mut WidgetIdMap<NodeSnapshot>) {
        let total = self.snaps.len() as u32;
        if total >= COMPACT_MIN_TOTAL
            && self.orphaned.saturating_mul(4) >= total * COMPACT_ORPHAN_RATIO_NUM
        {
            self.compact(forest, prev);
        }
    }
}

impl DamageEngine {
    /// Drop the per-widget previous-frame snapshot map. Pairs with
    /// the caller passing `force_full = true` into the next
    /// `compute` so the diff repopulates the map from scratch but
    /// still returns `Damage::Full`. Called by `Ui::frame` when
    /// the surface changed, the previous frame wasn't acked, or
    /// it's the first frame.
    pub(crate) fn invalidate_prev(&mut self) {
        self.prev.clear();
        self.arena.clear();
    }

    /// Diff against the just-finished frame and return a
    /// [`Damage`] ready for the renderer:
    ///
    /// - [`Damage::Skip`] — empty region, nothing changed (also the
    ///   outcome for a degenerate zero-area surface, since every rect
    ///   surface-clips away).
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
    /// [`crate::forest::seen_ids::SeenIds`] so damage and `text` reuse
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
            cascades,
            surface,
            prev_time,
            now,
        } = input;
        // `force_full` is the "treat as a fresh frame" signal — set
        // by the caller when `Ui::classify_frame` decided
        // this frame must repaint everything (surface changed, last
        // frame wasn't acked, or first frame). Caller has already
        // called `invalidate_prev` to drop the per-widget snapshot
        // map; we still run the full diff pass to repopulate it for
        // next frame, just return `Damage::Full` instead of the
        // filtered region.
        #[cfg(any(test, feature = "internals"))]
        {
            self.dirty.clear();
            self.subtree_skips = 0;
        }

        // ── Pass 1: collect raw rects ─────────────────────────────
        //
        // Every damage source pushes its contributions into
        // `self.raw_rects` without applying the merge or budget
        // policy. Sources: structural diff (added / hash-changed /
        // removed widget), predamaged anim rects, and the
        // `removed`-set eviction tail. Pass 2 collapses the buffer
        // into the bounded region.
        self.raw_rects.clear();

        // Alias each mutated field once so the diff body can name
        // them independently — Entry holds the borrow on `prev` only,
        // leaving `arena` / `raw_rects` free.
        let prev_map = &mut self.prev;
        let arena = &mut self.arena;
        let raw_rects = &mut self.raw_rects;

        #[cfg(any(test, feature = "internals"))]
        let dirty_out = &mut self.dirty;
        #[cfg(any(test, feature = "internals"))]
        let subtree_skips_out = &mut self.subtree_skips;

        for (layer, tree) in forest.iter_paint_order() {
            let layer_cascades = &cascades.layers[layer];
            let cascade_inputs = layer_cascades.cascade_inputs.as_slice();
            let n = tree.records.len();
            let widget_ids = tree.records.widget_id();
            let subtree_end = tree.records.subtree_end();
            let layer_paints = &layer_cascades.paint_arena.rows;
            let layer_node_paints = &layer_cascades.paint_arena.node_spans;
            let mut i = 0;
            while i < n {
                let node_span = layer_node_paints[i];
                let curr_paints_slice = &layer_paints[node_span.range()];
                let curr_paints = node_span.len > 0;
                let curr_node_hash = tree.rollups.node[i];
                let curr_subtree_hash = tree.rollups.subtree[i];
                let curr_cascade_input = cascade_inputs[i];
                // This node's next-frame snapshot — every field but
                // `paint_span` is fixed per node, so the arms differ
                // only in which span they pass.
                let make_snapshot = |paint_span| NodeSnapshot {
                    paint_span,
                    hash: curr_node_hash,
                    subtree_hash: curr_subtree_hash,
                    cascade_input: curr_cascade_input,
                };
                let advance = match prev_map.entry(widget_ids[i]) {
                    // Skip the snapshot insert for a new node that paints
                    // nothing or paints entirely off-surface. `union_screens`
                    // is the former `Cascade.paint_rect`, recomputed here on
                    // the cold (Vacant) path rather than stored per node.
                    Entry::Vacant(_)
                        if !union_screens(curr_paints_slice)
                            .is_some_and(|u| u.intersects(surface)) =>
                    {
                        1
                    }
                    Entry::Vacant(e) => {
                        let paint_span = arena.append(curr_paints_slice);
                        push_screens(raw_rects, curr_paints_slice);
                        e.insert(make_snapshot(paint_span));
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(NodeId(i as u32));
                        1
                    }
                    // Tier 1 — whole-subtree skip. `subtree_hash` rolls
                    // up this node's own `node_hash`, so a match already
                    // implies the node itself is unchanged; paired with an
                    // unchanged `cascade_input` (own `layout_rect` +
                    // ancestor state) every descendant is bit-identical by
                    // induction. Cheapest high-value check — the dominant
                    // steady-state path skips the whole tree at the root —
                    // so it goes first.
                    Entry::Occupied(e)
                        if e.get().subtree_hash == curr_subtree_hash
                            && e.get().cascade_input == curr_cascade_input =>
                    {
                        let span = (subtree_end[i].end() as usize) - i;
                        #[cfg(any(test, feature = "internals"))]
                        if span > 1 {
                            *subtree_skips_out += 1;
                        }
                        span
                    }
                    // Tier 2 — node's own authoring + cascade state
                    // unchanged but `subtree_hash` differs, so a descendant
                    // changed. Own paints are identical (`hash` +
                    // `cascade_input` equal ⇒ identical screens), so the
                    // arena slots stay correct; just refresh the rollup and
                    // descend.
                    Entry::Occupied(mut e)
                        if e.get().hash == curr_node_hash
                            && e.get().cascade_input == curr_cascade_input =>
                    {
                        e.get_mut().subtree_hash = curr_subtree_hash;
                        1
                    }
                    Entry::Occupied(mut e) if curr_paints => {
                        let prev = *e.get();
                        let leg =
                            arena.diff_changed_leg(raw_rects, prev.paint_span, curr_paints_slice);
                        // The per-shape diff covers shapes that moved or
                        // changed. When it found every `Paint` identical
                        // (`geometry_unchanged`) it emitted nothing — and
                        // only a `cascade_input` change (ancestor disable,
                        // clip-saturated pan, visibility toggle) can alter
                        // this node's pixels while leaving every `Paint`
                        // bit-identical, so we repaint the union only then.
                        // A pure `node_hash` flip with unchanged
                        // `cascade_input` means the authoring stream
                        // differed without touching own pixels — most
                        // commonly a child added/removed (the per-child
                        // marker folded into `node_hash` by
                        // `compute_hashes`), already covered by the
                        // subtree/eviction diff. Repainting the union there
                        // spuriously re-damages every direct shape — e.g.
                        // all canvas connections when an unrelated node is
                        // deleted.
                        if leg.geometry_unchanged
                            && prev.cascade_input != curr_cascade_input
                            && let Some(u) = union_screens(curr_paints_slice)
                        {
                            raw_rects.push(u);
                        }
                        *e.get_mut() = make_snapshot(leg.span);
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(NodeId(i as u32));
                        1
                    }
                    Entry::Occupied(e) => {
                        // Painting → non-painting transition: push
                        // everything the node *was* painting, then
                        // evict.
                        let prev = *e.get();
                        push_screens(raw_rects, &arena.snaps[prev.paint_span.range()]);
                        arena.mark_orphaned(prev.paint_span.len);
                        e.remove();
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(NodeId(i as u32));
                        1
                    }
                };
                i += advance;
            }
        }

        // Structural diff has populated `self.prev` for next frame's
        // baseline; on `force_full` everything downstream just builds
        // a region we'd discard, so short-circuit here. The removed
        // eviction tail is a no-op in this branch (caller already
        // cleared `self.prev` via `invalidate_prev`), and the anim
        // iterator is lazy — dropping it without consuming is free.
        if force_full {
            return Damage::Full;
        }

        // Predamaged anim rects. The structural diff above is
        // content-only and (intentionally) doesn't pick up phase
        // flips — bumping `node_hash` / `subtree_hash` would
        // invalidate MeasureCache for the owner's ancestor chain on
        // every flip even though layout didn't change. The encoder's
        // `PaintAnims::sample` decides per-rect whether to emit a
        // quad (visible half) or skip (hidden half).
        extend_predamaged(&mut self.raw_rects, forest, cascades, prev_time, now);

        // Removed-widget eviction tail. Every remaining `prev` entry
        // painted last frame (invariant), so its parts always
        // contribute. Push decomposed — chrome + per-shape — so a
        // multi-shape owner going away pushes its actual painted
        // footprint, not the union of disjoint shapes plus the gaps
        // between them.
        for wid in removed {
            if let Some(snap) = self.prev.remove(wid) {
                push_screens(
                    &mut self.raw_rects,
                    &self.arena.snaps[snap.paint_span.range()],
                );
                self.arena.mark_orphaned(snap.paint_span.len);
            }
        }

        // Reclaim the arena once orphaned slots exceed the threshold.
        self.arena.maybe_compact(forest, &mut self.prev);

        // ── Pass 2: collapse to the bounded region ────────────────
        self.finish_region(surface)
    }

    /// Pass 2: collapse the accumulated `raw_rects` into a budgeted
    /// region and lift it to a [`Damage`]. Shared tail of both compute
    /// paths.
    fn finish_region(&self, surface: Rect) -> Damage {
        let region = DamageRegion::collapse_from(&self.raw_rects, self.budget_px, surface);
        Damage::new(surface, region)
    }

    /// PaintOnly fast path. The tree wasn't rebuilt this frame, so
    /// every node would match its prev snapshot and contribute nothing
    /// to the structural diff — skip Pass 1 entirely. Only the
    /// caller-supplied predamaged anim rects matter.
    pub(crate) fn compute_paint_only(&mut self, input: DamageInput<'_>) -> Damage {
        #[cfg(any(test, feature = "internals"))]
        {
            self.dirty.clear();
            self.subtree_skips = 0;
        }
        self.raw_rects.clear();
        extend_predamaged(
            &mut self.raw_rects,
            input.forest,
            input.cascades,
            input.prev_time,
            input.now,
        );
        self.finish_region(input.surface)
    }
}

/// Drain every paint's screen rect into the raw-rect buffer. Used by
/// the Vacant-insert arm (everything's new), the eviction arm
/// (everything's going), and the removed-widget tail.
#[inline]
fn push_screens(out: &mut Vec<Rect>, paints: &[Paint]) {
    for p in paints {
        out.push(p.screen);
    }
}

/// Screen-space union of a node's paint rows — the node's own paint
/// extent, formerly stored as `Cascade.paint_rect`. The cascade no
/// longer caches it; the damage diff recomputes it here on its cold
/// paths (the Vacant surface-cull and the tier-3 cascade-state union
/// push) from the same `paint_arena` slice those arms already touch.
/// `None` for a non-painting node (empty slice).
#[inline]
fn union_screens(paints: &[Paint]) -> Option<Rect> {
    let mut it = paints.iter();
    let first = it.next()?.screen;
    Some(it.fold(first, |acc, p| acc.union(p.screen)))
}

fn extend_predamaged(
    out: &mut Vec<Rect>,
    forest: &Forest,
    cascades: &Cascades,
    prev_time: Option<Duration>,
    now: Duration,
) {
    // No prev frame ⇒ Pass 1 already contributed every painting
    // widget's rect (every entry was Vacant), and a paint-anim rect
    // is always a sub-rect of its owner — nothing new to add.
    let Some(prev) = prev_time else { return };
    for (layer, tree) in forest.iter_paint_order() {
        let arena = &cascades.layers[layer].paint_arena;
        let paints = &arena.rows;
        let node_spans = &arena.node_spans;
        let shape_spans = tree.records.shape_span();
        for e in &tree.paint_anims.entries {
            if e.anim.next_wake(prev) > now {
                continue;
            }
            // shape ordinal within the owner's `shape_span`, then map
            // to a paint slot inside the owner's `node_span`. Chrome
            // (when present) sits at row 0 of the node span, shape
            // paints follow in record order, so the chrome offset is
            // `1` when this node has chrome — same layout
            // `compute_paint_rect` emits.
            let node_idx = e.node_idx as usize;
            let owner_shapes = shape_spans[node_idx];
            let ordinal = e.shape_idx - owner_shapes.start;
            let chrome_offset = u32::from(tree.chrome(NodeId(e.node_idx)).is_some());
            let node_span = node_spans[node_idx];
            let want = chrome_offset + ordinal;
            // Cascade may not have emitted a paint for this shape —
            // `compute_paint_rect` skips the per-shape Paint row when
            // the subtree's cascade walk did so (e.g. invisible-subtree
            // owners with `len == 0`). Treat that as "no anim damage".
            if want < node_span.len {
                let paint_idx = node_span.start + want;
                out.push(paints[paint_idx as usize].screen);
            }
        }
    }
}

/// In-tree-test-only reach-in. Lives in a plain `#[cfg(test)]` impl
/// (not the `internals`-gated `test_support` mod) because only the
/// crate's own unit tests call it — so it needs no `allow(dead_code)`
/// for the feature-only build.
#[cfg(test)]
impl DamageEngine {
    /// Union of the paint screens retained for `wid` last frame — the
    /// value the (now removed) `NodeSnapshot.rect` field used to cache.
    /// Equal to the node's `Cascade.paint_rect`. `None` when `wid`
    /// didn't paint last frame (no `prev` entry).
    pub(crate) fn prev_paint_rect(&self, wid: WidgetId) -> Option<Rect> {
        let snap = self.prev.get(&wid)?;
        let mut screens = self.arena.snaps[snap.paint_span.range()].iter();
        let first = screens.next()?.screen;
        Some(screens.fold(first, |acc, p| acc.union(p.screen)))
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    use crate::forest::Forest;
    use crate::ui::damage::{DamageEngine, PaintSnapArena};

    impl PaintSnapArena {
        /// Live entries in the arena (sum of every live
        /// `paint_span.len`, plus orphaned tail). Introspection only.
        #[inline]
        pub(crate) fn len(&self) -> usize {
            self.snaps.len()
        }

        /// Orphan count — drives the compaction trigger.
        #[inline]
        pub(crate) fn orphaned(&self) -> u32 {
            self.orphaned
        }

        /// How many times [`Self::compact`] has run since
        /// construction.
        #[inline]
        pub(crate) fn compactions_run(&self) -> u32 {
            self.compactions_run
        }
    }

    impl DamageEngine {
        /// Force a compaction pass. Production frames go through
        /// `compute`, which calls `arena.maybe_compact` after the
        /// eviction tail; this is the entry point for tests / benches
        /// that want to drive the compaction directly. The `internals`
        /// feature exposes this for downstream consumers even though
        /// only `cfg(test)` callers exist today — keep `allow(dead_code)`
        /// so a feature-only build doesn't trip `-D warnings`.
        #[allow(dead_code)]
        pub(crate) fn compact_paint_snaps(&mut self, forest: &Forest) {
            self.arena.compact(forest, &mut self.prev);
        }
    }
}

#[cfg(test)]
mod tests;
