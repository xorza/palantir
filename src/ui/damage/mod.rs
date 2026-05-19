//! Per-frame damage detection. Computed in [`Ui::post_record`] after
//! `compute_hashes`; rebuilds the prev-frame snapshot in the same
//! pass via the `entry()` API — vacant slots get inserted, occupied
//! slots get diffed and either updated or evicted.
//!
//! A node is **dirty** if its `(rect, authoring-hash)` differs from
//! the entry keyed by the same `WidgetId` in `DamageEngine.prev`, OR it
//! had no entry (added). A `WidgetId` present in `DamageEngine.prev` with
//! no matching node this frame contributes its prev rect (removed).
//! Each contribution is folded into a [`region::DamageRegion`] via
//! its merge policy; the result drives the encoder filter and the
//! per-pass scissor list in the backend.
//!
//! **Painting-only invariant.** `DamageEngine.prev` only holds entries for
//! widgets that painted on their last recorded frame (have chrome OR
//! direct shapes — see `Tree.rollups.paints`). Non-painting nodes
//! contribute zero pixels, so they're skipped on insert. A
//! painting→non-painting transition evicts the entry in the same
//! diff loop; the prev rect contributes (clear those pixels), the
//! curr rect doesn't.
//!
//! `DamageEngine.dirty` is the per-node dirty list (added /
//! hash-changed / rect-changed) in pre-order paint order. It's
//! gated behind `cfg(any(test, feature = "internals"))` — production
//! builds skip the per-node `Vec::push` entirely; tests and benches
//! assert on it through this gate.

use crate::forest::Forest;
use crate::forest::rollups::{CascadeInputHash, NodeHash};
use crate::forest::tree::{NodeId, Tree, TreeItem, TreeItems};
use crate::primitives::approx::EPS;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::Cascades;
use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::hash_map::Entry;
use std::time::Duration;

pub mod region;

/// Per-shape entry stored in [`DamageEngine::shape_snaps`]. Each
/// direct shape of a painting widget snapshots its screen rect and
/// canonical hash (computed at `Shapes::add` time) so the damage diff
/// can push the pair (prev, curr) per *changed* shape instead of the
/// owner's whole `paint_rect` union — the optimisation that flips a
/// multi-bezier graph canvas from "drag one node ⇒ damage covers all
/// curves" to "drag one node ⇒ damage covers only the curves actually
/// touching it." Indexed positionally by `(WidgetId, ordinal)`: the
/// n-th `add_shape` call in the owner's body. Ordinal stability
/// across frames depends on deterministic authoring order; ordinal
/// shifts degrade gracefully (the affected tail looks like
/// insertions+deletions, which still produce correct, if coarser,
/// damage rects).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ShapeSnap {
    pub(crate) rect: Rect,
    pub(crate) hash: NodeHash,
}

/// Per-painting-widget snapshot held in [`DamageEngine::prev`], keyed by
/// stable [`WidgetId`]. Only widgets that painted last frame have an
/// entry — non-painting nodes (e.g. a popup's invisible click-eater)
/// are skipped on insert, so their full-surface rect can't trip the
/// full-repaint coverage threshold on add or remove.
///
/// **Storage shape.** Per-shape snapshots don't live inline here —
/// they live in [`DamageEngine::shape_snaps`], a single contiguous
/// arena shared by every painting widget, and this struct just holds
/// a `Span` into it. Eliminates the per-widget heap header / inline-
/// fallback overhead of the previous `TinyVec<[ShapeSnap; 1]>` field.
/// Subtree-skip stays free (snapshots in the skipped subtree retain
/// their span — no per-frame rewrite). Span churn (a shape was
/// added or removed mid-list) orphans the old slice in the arena;
/// [`DamageEngine::maybe_compact_shape_snaps`] periodically reseats
/// live spans into a fresh buffer once orphaned bytes pass a
/// threshold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// Screen-space rect from last frame's `Cascade.paint_rect`
    /// (raw transformed rect inflated by per-shape ink overhang —
    /// drop-shadow halos — then intersected with the ancestor clip).
    /// Using `paint_rect` rather than `visible_rect` means a node
    /// going away (e.g. on tab switch) contributes the full halo
    /// it painted last frame, so the encoder clears the shadow bleed.
    ///
    /// Kept as the union (chrome ∪ shapes) so the Occupied-equal arm's
    /// fast check stays `e.rect == curr_paint_rect`. The decomposition
    /// is recovered via `chrome_rect` + `shape_span` indexing into
    /// `DamageEngine::shape_snaps`.
    pub(crate) rect: Rect,
    /// Chrome-only screen rect (background + shadow halo). Sister to
    /// `Cascades.chrome_rects`. Sentinel value is `Rect::ZERO` for
    /// chromeless nodes, but the *canonical* chromedness predicate
    /// is `chrome_hash != NodeHash::default()` — the diff and
    /// `push_decomposed_paint` key on the hash, never on the rect's
    /// area. A chromed-but-clipped-to-nothing node legitimately has
    /// `chrome_rect == Rect::ZERO` with a non-default `chrome_hash`.
    pub(crate) chrome_rect: Rect,
    /// Chrome-only authoring hash. Paired with `chrome_rect` so the
    /// damage diff knows whether to push chrome's rect pair: rect
    /// alone catches positional shifts, hash catches authoring
    /// flips (fill / stroke / shadow / radius) that keep the rect
    /// identical (hover fill changes are the canonical case).
    /// `NodeHash::default()` for chromeless nodes — sister to
    /// `Tree.rollups.chrome`. Also serves as the "this node has
    /// chrome" predicate (vs `chrome_rect.area() > 0`, which is
    /// overloaded with "clipped to nothing").
    pub(crate) chrome_hash: NodeHash,
    /// Slice into [`DamageEngine::shape_snaps`] describing this
    /// widget's per-shape snapshots in record order. Empty span for
    /// zero-shape painters (chrome-only).
    pub(crate) shape_span: Span,
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

/// Output of one frame's damage pass plus the cross-frame state it
/// reads to produce that output.
///
/// `dirty` lists every added / hash-changed / rect-changed node in
/// pre-order paint order (test-only). `region` accumulates the
/// per-rect contributions (added node's curr rect, changed node's
/// prev + curr, removed widget's prev rect) through
/// [`region::DamageRegion::add`]'s merge policy — empty region ⇒
/// nothing changed, so the host-requested redraw maps to
/// [`Damage::Skip`].
///
/// `prev` is the per-`WidgetId` snapshot map carried over from last
/// frame; it's mutated in place during `compute` (read old, write
/// new) so steady-state frames don't allocate.
///
/// Capacities on `dirty` and `prev` are retained across frames;
/// `region` is inline (`DamageRegion` is `Copy`).
pub(crate) struct DamageEngine {
    #[cfg(any(test, feature = "internals"))]
    pub(crate) dirty: Vec<NodeId>,
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
    pub(crate) prev: FxHashMap<WidgetId, NodeSnapshot>,
    /// Per-painting-widget shape snapshots, packed contiguously. Each
    /// `NodeSnapshot` holds a `shape_span` slice into this buffer.
    /// Append-only writes for count-change paths (new span at end,
    /// old slice orphaned); in-place writes for same-count refreshes.
    /// [`Self::maybe_compact_shape_snaps`] reseats live spans into
    /// `shape_snaps_scratch` once orphaned bytes exceed half the
    /// buffer, then swaps. Retained capacity — steady-state alloc-
    /// free even under shape-count churn.
    pub(crate) shape_snaps: Vec<ShapeSnap>,
    /// Reusable destination for compaction (and a swap target). Same
    /// invariants as `shape_snaps` after a `swap`. Kept around so
    /// compaction itself doesn't allocate on the hot path.
    pub(crate) shape_snaps_scratch: Vec<ShapeSnap>,
    /// Number of `ShapeSnap` entries in `shape_snaps` that no live
    /// `NodeSnapshot::shape_span` points into — accumulates when a
    /// widget's shape count grows past prev (the in-place updates
    /// are lifted to the tail and the old slots become orphans),
    /// when a widget's shape count shrinks (the tail of its prev
    /// span goes unreferenced), or when a widget is evicted. Drives
    /// the compaction trigger. Counted in entries, not bytes.
    pub(crate) shape_snaps_orphaned: u32,
    /// Compaction-event counter — bumped each time
    /// `compact_shape_snaps` runs. Gated behind `internals` so
    /// benches can verify the path is actually exercised; production
    /// builds skip the field entirely.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) compactions_run: u32,
    /// Pass-1 scratch buffer. `compute` walks every damage source
    /// (structural diff, predamaged anim rects, removed-widget evict)
    /// and appends each contribution here without applying the merge
    /// policy. Pass 2 hands this slice to `DamageRegion::collapse_from`
    /// which produces the bounded region. Retained capacity — no
    /// per-frame allocation in steady state.
    pub(crate) raw_rects: Vec<Rect>,
    /// Count of subtree-skip jumps the last `compute` performed —
    /// every match of the Occupied-equal arm jumped `subtree_end - i`
    /// instead of advancing by 1. Read by tests and benches via
    /// `support::internals::damage_subtree_skips`; zero on first
    /// frame and on full-repaint fall-through. Gated alongside
    /// `dirty` — production builds don't pay the increment.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) subtree_skips: u32,
}

impl Default for DamageEngine {
    fn default() -> Self {
        Self {
            #[cfg(any(test, feature = "internals"))]
            dirty: Vec::new(),
            budget_px: DEFAULT_PASS_BUDGET_PX,
            prev: FxHashMap::default(),
            shape_snaps: Vec::new(),
            shape_snaps_scratch: Vec::new(),
            shape_snaps_orphaned: 0,
            #[cfg(any(test, feature = "internals"))]
            compactions_run: 0,
            raw_rects: Vec::new(),
            #[cfg(any(test, feature = "internals"))]
            subtree_skips: 0,
        }
    }
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
    None,
    Full,
    Partial(DamageRegion),
}

impl Damage {
    /// True iff this is the skip signal — caller can short-circuit
    /// the renderer entirely.
    #[inline]
    pub(crate) fn is_none(self) -> bool {
        matches!(self, Damage::None)
    }

    pub(crate) fn new(surface: Rect, region: DamageRegion) -> Damage {
        if region.is_empty() {
            return Damage::None;
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

impl DamageEngine {
    /// Drop the per-widget previous-frame snapshot map. Pairs with
    /// the caller passing `force_full = true` into the next
    /// `compute` so the diff repopulates the map from scratch but
    /// still returns `Damage::Full`. Called by `Ui::pre_record` when
    /// the surface changed, the previous frame wasn't acked, or
    /// it's the first frame.
    pub(crate) fn invalidate_prev(&mut self) {
        self.prev.clear();
        self.shape_snaps.clear();
        self.shape_snaps_orphaned = 0;
    }

    /// Walk live `NodeSnapshot::shape_span`s in pre-order paint
    /// order — same order the next frame's diff body visits them —
    /// and reseat into `shape_snaps_scratch`, then swap. Subsequent
    /// per-shape comparisons hit sequential cache lines instead of
    /// the scattered HashMap-order layout that
    /// `prev.values_mut()` would produce.
    ///
    /// Walks `forest` rather than the prev map directly: visiting
    /// every node is O(N), but typical N is the same order as the
    /// painting-widget count, and the locality win on next-frame
    /// reads more than pays the bookkeeping. The diff and the
    /// compaction sweep share one canonical order.
    fn compact_shape_snaps(&mut self, forest: &Forest) {
        self.shape_snaps_scratch.clear();
        for (_layer, tree) in forest.iter_paint_order() {
            for wid in tree.records.widget_id() {
                let Some(snap) = self.prev.get_mut(wid) else {
                    continue;
                };
                if snap.shape_span.len == 0 {
                    // Chrome-only / shape-less owner: nothing to copy,
                    // but the stale `start` still points into the
                    // pre-swap buffer. Normalize so the post-swap
                    // `shape_snaps[span.range()]` (in the removed-
                    // eviction loop, etc.) stays in bounds even though
                    // the slice it yields is empty.
                    snap.shape_span = Span::new(0, 0);
                    continue;
                }
                let new_start = self.shape_snaps_scratch.len() as u32;
                self.shape_snaps_scratch
                    .extend_from_slice(&self.shape_snaps[snap.shape_span.range()]);
                snap.shape_span = Span::new(new_start, snap.shape_span.len);
            }
        }
        std::mem::swap(&mut self.shape_snaps, &mut self.shape_snaps_scratch);
        self.shape_snaps_orphaned = 0;
        #[cfg(any(test, feature = "internals"))]
        {
            self.compactions_run = self.compactions_run.saturating_add(1);
        }
    }

    /// Trigger compaction when:
    /// 1. the arena is large enough that the O(N) reseat walk is
    ///    cheaper than letting the buffer drift (`MIN_TOTAL`), and
    /// 2. orphaned entries are ≥ 75 % of the buffer
    ///    (`orphaned * 4 >= total * 3`). Up from the previous 50 %.
    ///    Halves compaction frequency at the cost of letting the
    ///    buffer drift ~2× larger before reclamation — net win on
    ///    `shape_churn_full` where every frame produced compaction
    ///    pressure under the old threshold.
    fn maybe_compact_shape_snaps(&mut self, forest: &Forest) {
        const MIN_TOTAL: u32 = 256;
        let total = self.shape_snaps.len() as u32;
        if total >= MIN_TOTAL && self.shape_snaps_orphaned.saturating_mul(4) >= total * 3 {
            self.compact_shape_snaps(forest);
        }
    }

    /// Diff against the just-finished frame and return a
    /// [`Damage`] ready for the renderer:
    ///
    /// - [`Damage::Skip`] — empty region, nothing changed.
    /// - [`Damage::Partial`] — coverage below
    ///   [`FULL_REPAINT_THRESHOLD`].
    /// - [`Damage::Full`] — first frame / surface change /
    ///   degenerate surface / coverage above the threshold.
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
    /// Rects are tracked in **screen space** (read straight off
    /// `Cascade.paint_rect` — the transformed layout rect inflated by
    /// per-shape ink overhang, then ancestor-clipped). This makes
    /// damage match where the GPU actually paints, so the backend
    /// scissor lands on the right pixels even under transformed
    /// parents or around a drop shadow.
    ///
    /// `surface` is the rect the host arranged the UI into this
    /// frame. A degenerate zero-area surface short-circuits to full
    /// repaint; it shouldn't happen in practice (host filters
    /// resize-to-zero), but cheap to handle.
    #[profiling::function]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compute(
        &mut self,
        forest: &Forest,
        cascades: &Cascades,
        removed: &FxHashSet<WidgetId>,
        surface: Rect,
        force_full: bool,
        prev_time: Option<Duration>,
        now: Duration,
    ) -> Damage {
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
        // leaving `shape_snaps` / `raw_rects` / `orphaned` free.
        let prev_map = &mut self.prev;
        let shape_snaps = &mut self.shape_snaps;
        let orphaned = &mut self.shape_snaps_orphaned;
        let raw_rects = &mut self.raw_rects;
        #[cfg(any(test, feature = "internals"))]
        let dirty_out = &mut self.dirty;
        #[cfg(any(test, feature = "internals"))]
        let subtree_skips_out = &mut self.subtree_skips;

        for (layer, tree) in forest.iter_paint_order() {
            let rows = cascades.rows_for(layer);
            let n = tree.records.len();
            let widget_ids = tree.records.widget_id();
            let subtree_end = tree.records.subtree_end();
            let layer_chrome_rects = &cascades.chrome_rects[layer.idx()];
            let layer_shape_rects = &cascades.shape_rects[layer.idx()];
            let shape_hashes = tree.shapes.hashes.as_slice();
            let chrome_hashes = tree.rollups.chrome.as_slice();
            let mut i = 0;
            while i < n {
                // Invariant: `self.prev` only holds entries for widgets
                // that painted last frame. Non-painting nodes contribute
                // zero rect, so a never-painted widget needs no snapshot
                // and a painting→non-painting transition evicts the
                // entry.
                //
                // Two unchanged predicates, *not* the same:
                //
                // - **No-work** (`rect` + `node_hash` match): this node's
                //   own paint contribution is identical. Falls through to
                //   the next node — descendants may still have changed.
                // - **Subtree-skip** (additionally `subtree_hash` +
                //   `cascade_input` match): the whole subtree is
                //   bit-identical. Every descendant's `(paint_rect,
                //   node_hash)` matches prev by induction — no update,
                //   no rect contribution, jump to `subtree_end[i]`.
                //
                // The split matters: if merged, an internal node with
                // a stable `(rect, node_hash)` but a child that changed
                // colour would fail the merged predicate (its
                // `subtree_hash` rolled the child's new hash), fall into
                // the "changed" arm, and contribute its own (unchanged)
                // rect — bloating damage from the child's leaf rect to
                // the whole parent's rect.
                let curr = CurrNode {
                    node: NodeId(i as u32),
                    rect: rows[i].paint_rect,
                    paints: tree.rollups.paints.contains(i),
                    node_hash: tree.rollups.node[i],
                    subtree_hash: tree.rollups.subtree[i],
                    cascade_input: rows[i].cascade_input,
                    chrome_rect: layer_chrome_rects[i],
                    chrome_hash: chrome_hashes[i],
                };
                let advance = match prev_map.entry(widget_ids[i]) {
                    // First-seen non-painter or first-seen painter
                    // whose entire `paint_rect` lies off the surface:
                    // nothing visible to push, no value in seeding
                    // `prev` (next-frame diff would just see it vanish
                    // without anyone caring). The surface-clip at
                    // `DamageRegion::collapse_from` would drop
                    // `curr.rect` anyway — this just sidesteps the
                    // hashmap insert for nodes that pan/zoom landed
                    // outside the viewport.
                    Entry::Vacant(_) if !curr.paints || !curr.rect.intersects(surface) => 1,
                    Entry::Vacant(e) => {
                        let shape_span = append_curr_shape_snaps(
                            shape_snaps,
                            tree,
                            curr.node,
                            layer_shape_rects,
                            shape_hashes,
                        );
                        push_decomposed_paint(
                            raw_rects,
                            curr.chrome_hash,
                            curr.chrome_rect,
                            &shape_snaps[shape_span.range()],
                        );
                        e.insert(curr.to_snapshot(shape_span));
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(curr.node);
                        1
                    }
                    Entry::Occupied(mut e)
                        if e.get().rect == curr.rect && e.get().hash == curr.node_hash =>
                    {
                        let prev = *e.get();
                        if prev.subtree_hash == curr.subtree_hash
                            && prev.cascade_input == curr.cascade_input
                        {
                            // Subtree-skip: the whole subtree is
                            // bit-identical. Spans into `shape_snaps`
                            // stay valid; nothing to update.
                            let span = (subtree_end[i] as usize) - i;
                            #[cfg(any(test, feature = "internals"))]
                            if span > 1 {
                                *subtree_skips_out += 1;
                            }
                            span
                        } else {
                            // Own paint authoring unchanged (`rect` +
                            // `node_hash` match) but cascade input or
                            // subtree-rollup shifted. Direct shapes'
                            // tessellated pixels moved with the
                            // ancestor transform even though our
                            // clipped union didn't — push the union
                            // (matches the pre-decomposition contract:
                            // clipped paint_rect is already covered,
                            // but new positions need a repaint). Then
                            // refresh per-shape rects in place — count
                            // unchanged because node_hash matched
                            // (shape set identical by induction).
                            if curr.paints && prev.cascade_input != curr.cascade_input {
                                raw_rects.push(curr.rect);
                            }
                            refresh_shape_rects_in_arena(
                                shape_snaps,
                                prev.shape_span,
                                tree,
                                curr.node,
                                layer_shape_rects,
                            );
                            *e.get_mut() = curr.to_snapshot(prev.shape_span);
                            1
                        }
                    }
                    Entry::Occupied(mut e) if curr.paints => {
                        let prev = *e.get();
                        push_changed_chrome(raw_rects, &prev, &curr);
                        let new_span = diff_changed_shape_leg(
                            shape_snaps,
                            raw_rects,
                            orphaned,
                            prev.shape_span,
                            tree,
                            curr.node,
                            layer_shape_rects,
                            shape_hashes,
                        );
                        *e.get_mut() = curr.to_snapshot(new_span);
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(curr.node);
                        1
                    }
                    Entry::Occupied(e) => {
                        // Painting → non-painting transition: push
                        // everything the node *was* painting so the
                        // backbuffer at those pixels gets cleared,
                        // then evict.
                        let prev = *e.get();
                        push_decomposed_paint(
                            raw_rects,
                            prev.chrome_hash,
                            prev.chrome_rect,
                            &shape_snaps[prev.shape_span.range()],
                        );
                        *orphaned = orphaned.saturating_add(prev.shape_span.len);
                        e.remove();
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(curr.node);
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
                push_decomposed_paint(
                    &mut self.raw_rects,
                    snap.chrome_hash,
                    snap.chrome_rect,
                    &self.shape_snaps[snap.shape_span.range()],
                );
                self.shape_snaps_orphaned = self
                    .shape_snaps_orphaned
                    .saturating_add(snap.shape_span.len);
            }
        }

        // Reclaim the arena once orphaned slots exceed half the
        // buffer. Cheap walk amortised against the bytes saved.
        self.maybe_compact_shape_snaps(forest);

        // ── Pass 2: collapse to the bounded region ────────────────
        let region = DamageRegion::collapse_from(&self.raw_rects, self.budget_px, surface);
        Damage::new(surface, region)
    }

    /// PaintOnly fast path. The tree wasn't rebuilt this frame, so
    /// every node would match its prev snapshot and contribute nothing
    /// to the structural diff — skip Pass 1 entirely. Only the
    /// caller-supplied predamaged anim rects matter.
    pub(crate) fn compute_paint_only(
        &mut self,
        forest: &Forest,
        cascades: &Cascades,
        surface: Rect,
        prev_time: Option<Duration>,
        now: Duration,
    ) -> Damage {
        #[cfg(any(test, feature = "internals"))]
        {
            self.dirty.clear();
            self.subtree_skips = 0;
        }
        self.raw_rects.clear();
        extend_predamaged(&mut self.raw_rects, forest, cascades, prev_time, now);
        let region = DamageRegion::collapse_from(&self.raw_rects, self.budget_px, surface);
        Damage::new(surface, region)
    }
}

/// Per-node inputs to the diff body, packed at the top of each
/// iteration so the four match arms can name a single value instead
/// of seven loose locals. All fields are `Copy` and small; the struct
/// is build-and-drop per iteration with no heap traffic.
struct CurrNode {
    node: NodeId,
    rect: Rect,
    paints: bool,
    node_hash: NodeHash,
    subtree_hash: NodeHash,
    cascade_input: CascadeInputHash,
    chrome_rect: Rect,
    chrome_hash: NodeHash,
}

impl CurrNode {
    /// Build a `NodeSnapshot` by copying out fields and attaching
    /// the shape arena span. Used at every commit site (Vacant
    /// insert, refresh, changed arm) so the snapshot is constructed
    /// from one named value rather than six scattered field assigns
    /// through `get_mut`. Borrows `&self` — callers continue to use
    /// `curr.node` etc. afterwards without an explicit clone.
    fn to_snapshot(&self, shape_span: Span) -> NodeSnapshot {
        NodeSnapshot {
            rect: self.rect,
            chrome_rect: self.chrome_rect,
            chrome_hash: self.chrome_hash,
            shape_span,
            hash: self.node_hash,
            subtree_hash: self.subtree_hash,
            cascade_input: self.cascade_input,
        }
    }
}

/// Chrome leg of the Occupied-changed arm. Pushes the prev+curr
/// chrome rect pair when the rect moved OR the authoring hash flipped
/// (hover-fill is the canonical case: identical geometry, different
/// pixels). Chromedness is keyed on `chrome_hash != NodeHash::
/// default()` — chromeless nodes leave the slot at the default hash
/// and have `chrome_rect == Rect::ZERO`.
fn push_changed_chrome(out: &mut Vec<Rect>, prev: &NodeSnapshot, curr: &CurrNode) {
    if prev.chrome_rect == curr.chrome_rect && prev.chrome_hash == curr.chrome_hash {
        return;
    }
    if prev.chrome_hash != NodeHash::default() {
        out.push(prev.chrome_rect);
    }
    if curr.chrome_hash != NodeHash::default() {
        out.push(curr.chrome_rect);
    }
}

/// Per-shape diff leg of the Occupied-changed arm. In-place compare
/// against the prev span; only spill to the tail (and orphan the
/// prev slots) when shape count grew past `prev_len`. Shrink shortens
/// the span in place. Same-count common case writes straight back
/// into the existing span with no buffer manipulation.
///
/// Returns the new shape span. Bumps `orphaned` for prev slots that
/// no live snapshot references after the diff.
#[allow(clippy::too_many_arguments)]
fn diff_changed_shape_leg(
    arena: &mut Vec<ShapeSnap>,
    out: &mut Vec<Rect>,
    orphaned: &mut u32,
    prev_span: Span,
    tree: &Tree,
    node: NodeId,
    shape_rects: &[Rect],
    shape_hashes: &[NodeHash],
) -> Span {
    let prev_start = prev_span.start as usize;
    let prev_len = prev_span.len as usize;
    let mut ord = 0usize;
    let mut spilled_start: Option<u32> = None;
    for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
        let TreeItem::ShapeRecord(idx, _) = item else {
            continue;
        };
        let curr_shape = ShapeSnap {
            rect: shape_rects[idx as usize],
            hash: shape_hashes[idx as usize],
        };
        if spilled_start.is_some() {
            arena.push(curr_shape);
            out.push(curr_shape.rect);
        } else if ord < prev_len {
            let slot = &mut arena[prev_start + ord];
            if *slot != curr_shape {
                out.push(slot.rect);
                out.push(curr_shape.rect);
                *slot = curr_shape;
            }
        } else {
            // Growth: lift the in-place-updated prev_len entries to
            // the tail (canonical prefix of the new span), append
            // curr, switch to push-mode.
            let start = arena.len() as u32;
            arena.extend_from_within(prev_start..prev_start + prev_len);
            arena.push(curr_shape);
            out.push(curr_shape.rect);
            spilled_start = Some(start);
        }
        ord += 1;
    }
    match spilled_start {
        Some(start) => {
            *orphaned = orphaned.saturating_add(prev_len as u32);
            Span::new(start, ord as u32)
        }
        None if ord < prev_len => {
            // Vanished tail: push prev rects so their pixels get
            // repainted; shrink span in place. Slots `ord..prev_len`
            // become orphans inside the live buffer.
            for o in ord..prev_len {
                out.push(arena[prev_start + o].rect);
            }
            *orphaned = orphaned.saturating_add((prev_len - ord) as u32);
            Span::new(prev_span.start, ord as u32)
        }
        None => prev_span,
    }
}

/// Push the decomposed paint contribution of a snapshot (chrome rect +
/// each shape's rect) into `out`. Used by the Vacant-insert arm
/// (everything's new — push all parts) and the removed-widget
/// eviction tail (everything's going — push all parts). Chromedness
/// is keyed on `chrome_hash != NodeHash::default()` — the canonical
/// "this node has chrome authoring" predicate shared with the
/// Occupied-changed chrome leg.
fn push_decomposed_paint(
    out: &mut Vec<Rect>,
    chrome_hash: NodeHash,
    chrome_rect: Rect,
    shapes: &[ShapeSnap],
) {
    if chrome_hash != NodeHash::default() {
        out.push(chrome_rect);
    }
    for s in shapes {
        out.push(s.rect);
    }
}

/// Append one [`ShapeSnap`] per direct shape of `node` to the arena
/// in record order, returning the [`Span`] that covers them. Reads
/// `shape_rects` (screen-space) and `shape_hashes` (canonical,
/// computed at `Shapes::add` time). The `TreeItems` iterator already
/// filters to direct shapes only — same iterator the cascade and
/// encoder use, so the diff sees the same shape set as paint.
fn append_curr_shape_snaps(
    arena: &mut Vec<ShapeSnap>,
    tree: &Tree,
    node: NodeId,
    shape_rects: &[Rect],
    shape_hashes: &[NodeHash],
) -> Span {
    let start = arena.len() as u32;
    for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
        if let TreeItem::ShapeRecord(idx, _) = item {
            arena.push(ShapeSnap {
                rect: shape_rects[idx as usize],
                hash: shape_hashes[idx as usize],
            });
        }
    }
    let len = arena.len() as u32 - start;
    Span::new(start, len)
}

/// Cascade-input-shift refresh: each shape's screen `rect` moved with
/// the ancestor transform but its authoring hash didn't (the arm's
/// outer guard required `node_hash` to match, which folds every
/// shape's hash). Update the rects in place at the existing span;
/// count is guaranteed to match prev because the shape set itself is
/// bit-identical by induction.
fn refresh_shape_rects_in_arena(
    arena: &mut [ShapeSnap],
    span: Span,
    tree: &Tree,
    node: NodeId,
    shape_rects: &[Rect],
) {
    let start = span.start as usize;
    let mut ord = 0;
    for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
        if let TreeItem::ShapeRecord(idx, _) = item {
            if ord < span.len as usize {
                arena[start + ord].rect = shape_rects[idx as usize];
            }
            ord += 1;
        }
    }
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
        let shape_rects = &cascades.shape_rects[layer.idx()];
        for e in &tree.paint_anims.entries {
            if e.anim.next_wake(prev) <= now {
                out.push(shape_rects[e.shape_idx as usize]);
            }
        }
    }
}

#[cfg(test)]
mod tests;
