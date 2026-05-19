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
//! direct shapes — i.e. `cascades.layers[li].paint_arena.node_spans[i].len > 0`).
//! Non-painting nodes contribute zero pixels, so they're skipped on
//! insert. A painting→non-painting transition evicts the entry in the
//! same diff loop; the prev rects contribute (clear those pixels), the
//! curr rect doesn't.
//!
//! `DamageEngine.dirty` is the per-node dirty list (added /
//! hash-changed / rect-changed) in pre-order paint order. It's
//! gated behind `cfg(any(test, feature = "internals"))` — production
//! builds skip the per-node `Vec::push` entirely; tests and benches
//! assert on it through this gate.

use crate::forest::Forest;
use crate::forest::rollups::{CascadeInputHash, NodeHash};
use crate::forest::tree::NodeId;
use crate::primitives::approx::EPS;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::{Cascades, Paint};
use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use rustc_hash::{FxHashMap, FxHashSet};
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
/// they live in [`DamageEngine::paint_snaps`], a single contiguous
/// arena shared by every painting widget, and this struct just holds
/// a `Span` into it. Each row is either chrome (row 0 when present)
/// or one direct shape, mirroring `Cascades::paint_arenas`.
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
    /// fast check stays `e.rect == curr_paint_rect`. The per-row
    /// decomposition is recovered via `paint_span` indexing into
    /// `DamageEngine::paint_snaps`.
    pub(crate) rect: Rect,
    /// Slice into [`DamageEngine::paint_snaps`] describing this
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
    /// Per-painting-widget paint snapshots (chrome row 0 when present,
    /// then shapes), packed contiguously. Each `NodeSnapshot` holds a
    /// `paint_span` slice into this buffer. Append-only writes for
    /// count-change paths (new span at end, old slice orphaned);
    /// in-place writes for same-count refreshes.
    /// [`Self::maybe_compact_paint_snaps`] reseats live spans into
    /// `paint_snaps_scratch` once orphaned entries exceed the
    /// threshold, then swaps. Retained capacity — steady-state alloc-
    /// free even under paint-count churn.
    pub(crate) paint_snaps: Vec<Paint>,
    /// Reusable destination for compaction (and a swap target). Same
    /// invariants as `paint_snaps` after a `swap`.
    pub(crate) paint_snaps_scratch: Vec<Paint>,
    /// Number of `Paint` entries in `paint_snaps` that no live
    /// `NodeSnapshot::paint_span` points into. Drives the compaction
    /// trigger. Counted in entries, not bytes.
    pub(crate) paint_snaps_orphaned: u32,
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
    /// Compaction-event counter — bumped each time
    /// `compact_paint_snaps` runs. Gated behind `internals` so
    /// benches can verify the path is actually exercised.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) compactions_run: u32,
    #[cfg(any(test, feature = "internals"))]
    pub(crate) dirty: Vec<NodeId>,
}

impl Default for DamageEngine {
    fn default() -> Self {
        Self {
            #[cfg(any(test, feature = "internals"))]
            dirty: Vec::new(),
            budget_px: DEFAULT_PASS_BUDGET_PX,
            prev: FxHashMap::default(),
            paint_snaps: Vec::new(),
            paint_snaps_scratch: Vec::new(),
            paint_snaps_orphaned: 0,
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
        self.paint_snaps.clear();
        self.paint_snaps_orphaned = 0;
    }

    /// Walk live `NodeSnapshot::paint_span`s in pre-order paint
    /// order and reseat into `paint_snaps_scratch`, then swap.
    fn compact_paint_snaps(&mut self, forest: &Forest) {
        self.paint_snaps_scratch.clear();
        for (_layer, tree) in forest.iter_paint_order() {
            for wid in tree.records.widget_id() {
                let Some(snap) = self.prev.get_mut(wid) else {
                    continue;
                };
                if snap.paint_span.len == 0 {
                    continue;
                }
                let new_start = self.paint_snaps_scratch.len() as u32;
                self.paint_snaps_scratch
                    .extend_from_slice(&self.paint_snaps[snap.paint_span.range()]);
                snap.paint_span = Span::new(new_start, snap.paint_span.len);
            }
        }
        std::mem::swap(&mut self.paint_snaps, &mut self.paint_snaps_scratch);
        self.paint_snaps_orphaned = 0;
        #[cfg(any(test, feature = "internals"))]
        {
            self.compactions_run = self.compactions_run.saturating_add(1);
        }
    }

    /// Trigger compaction when the arena is large enough and orphaned
    /// entries are ≥ 75 % of the buffer.
    fn maybe_compact_paint_snaps(&mut self, forest: &Forest) {
        const MIN_TOTAL: u32 = 256;
        let total = self.paint_snaps.len() as u32;
        if total >= MIN_TOTAL && self.paint_snaps_orphaned.saturating_mul(4) >= total * 3 {
            self.compact_paint_snaps(forest);
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
        // leaving `paint_snaps` / `raw_rects` / `orphaned` free.
        let prev_map = &mut self.prev;
        let paint_snaps = &mut self.paint_snaps;
        let orphaned = &mut self.paint_snaps_orphaned;
        let raw_rects = &mut self.raw_rects;

        #[cfg(any(test, feature = "internals"))]
        let dirty_out = &mut self.dirty;
        #[cfg(any(test, feature = "internals"))]
        let subtree_skips_out = &mut self.subtree_skips;

        for (layer, tree) in forest.iter_paint_order() {
            let li = layer.idx();
            let layer_cascades = &cascades.layers[li];
            let rows = layer_cascades.rows.as_slice();
            let n = tree.records.len();
            let widget_ids = tree.records.widget_id();
            let subtree_end = tree.records.subtree_end();
            let layer_paints = &layer_cascades.paint_arena.rows;
            let layer_node_paints = &layer_cascades.paint_arena.node_spans;
            let mut i = 0;
            while i < n {
                let node_span = layer_node_paints[i];
                let curr_paints_slice = &layer_paints[node_span.range()];
                let curr = CurrNode {
                    node: NodeId(i as u32),
                    rect: rows[i].paint_rect,
                    paints: node_span.len > 0,
                    node_hash: tree.rollups.node[i],
                    subtree_hash: tree.rollups.subtree[i],
                    cascade_input: rows[i].cascade_input,
                };
                let advance = match prev_map.entry(widget_ids[i]) {
                    Entry::Vacant(_) if !curr.paints || !curr.rect.intersects(surface) => 1,
                    Entry::Vacant(e) => {
                        let paint_span = append_curr_paints(paint_snaps, curr_paints_slice);
                        push_screens(raw_rects, &paint_snaps[paint_span.range()]);
                        e.insert(curr.to_snapshot(paint_span));
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
                            let span = (subtree_end[i] as usize) - i;
                            #[cfg(any(test, feature = "internals"))]
                            if span > 1 {
                                *subtree_skips_out += 1;
                            }
                            span
                        } else {
                            if curr.paints && prev.cascade_input != curr.cascade_input {
                                raw_rects.push(curr.rect);
                            }
                            refresh_paint_rects_in_arena(
                                paint_snaps,
                                prev.paint_span,
                                curr_paints_slice,
                            );
                            *e.get_mut() = curr.to_snapshot(prev.paint_span);
                            1
                        }
                    }
                    Entry::Occupied(mut e) if curr.paints => {
                        let prev = *e.get();
                        let new_span = diff_changed_paint_leg(
                            paint_snaps,
                            raw_rects,
                            orphaned,
                            prev.paint_span,
                            curr_paints_slice,
                        );
                        *e.get_mut() = curr.to_snapshot(new_span);
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(curr.node);
                        1
                    }
                    Entry::Occupied(e) => {
                        // Painting → non-painting transition: push
                        // everything the node *was* painting, then
                        // evict.
                        let prev = *e.get();
                        push_screens(raw_rects, &paint_snaps[prev.paint_span.range()]);
                        *orphaned = orphaned.saturating_add(prev.paint_span.len);
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
                push_screens(
                    &mut self.raw_rects,
                    &self.paint_snaps[snap.paint_span.range()],
                );
                self.paint_snaps_orphaned = self
                    .paint_snaps_orphaned
                    .saturating_add(snap.paint_span.len);
            }
        }

        // Reclaim the arena once orphaned slots exceed the threshold.
        self.maybe_compact_paint_snaps(forest);

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
    #[cfg_attr(not(any(test, feature = "internals")), allow(dead_code))]
    node: NodeId,
    rect: Rect,
    paints: bool,
    node_hash: NodeHash,
    subtree_hash: NodeHash,
    cascade_input: CascadeInputHash,
}

impl CurrNode {
    fn to_snapshot(&self, paint_span: Span) -> NodeSnapshot {
        NodeSnapshot {
            rect: self.rect,
            paint_span,
            hash: self.node_hash,
            subtree_hash: self.subtree_hash,
            cascade_input: self.cascade_input,
        }
    }
}

/// Unified per-paint diff leg. In-place compare against the prev
/// span; spill to the tail (and orphan the prev slots) when the paint
/// count grew. Shrink shortens the span in place. Returns the new
/// paint span. Bumps `orphaned` for prev slots no live snapshot
/// references after the diff.
fn diff_changed_paint_leg(
    arena: &mut Vec<Paint>,
    out: &mut Vec<Rect>,
    orphaned: &mut u32,
    prev_span: Span,
    curr_paints: &[Paint],
) -> Span {
    let prev_start = prev_span.start as usize;
    let prev_len = prev_span.len as usize;
    let mut spilled_start: Option<u32> = None;
    for (ord, &curr_paint) in curr_paints.iter().enumerate() {
        if spilled_start.is_some() {
            arena.push(curr_paint);
            out.push(curr_paint.screen);
        } else if ord < prev_len {
            let slot = &mut arena[prev_start + ord];
            if *slot != curr_paint {
                out.push(slot.screen);
                out.push(curr_paint.screen);
                *slot = curr_paint;
            }
        } else {
            // Growth: lift the in-place-updated prev_len entries to
            // the tail, append curr, switch to push-mode.
            let start = arena.len() as u32;
            arena.extend_from_within(prev_start..prev_start + prev_len);
            arena.push(curr_paint);
            out.push(curr_paint.screen);
            spilled_start = Some(start);
        }
    }
    let ord = curr_paints.len();
    match spilled_start {
        Some(start) => {
            *orphaned = orphaned.saturating_add(prev_len as u32);
            Span::new(start, ord as u32)
        }
        None if ord < prev_len => {
            for o in ord..prev_len {
                out.push(arena[prev_start + o].screen);
            }
            *orphaned = orphaned.saturating_add((prev_len - ord) as u32);
            Span::new(prev_span.start, ord as u32)
        }
        None => prev_span,
    }
}

/// Append one [`Paint`] per row in `curr_paints` to the arena,
/// returning the [`Span`] that covers them.
fn append_curr_paints(arena: &mut Vec<Paint>, curr_paints: &[Paint]) -> Span {
    let start = arena.len() as u32;
    arena.extend_from_slice(curr_paints);
    Span::new(start, curr_paints.len() as u32)
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

/// Cascade-input-shift refresh: rects moved with the ancestor
/// transform but authoring hashes didn't (the arm's outer guard
/// required `node_hash` to match, which folds every paint's hash).
/// Update in place at the existing span; count is guaranteed to
/// match prev because the paint set is bit-identical by induction.
fn refresh_paint_rects_in_arena(arena: &mut [Paint], span: Span, curr_paints: &[Paint]) {
    let start = span.start as usize;
    let n = (span.len as usize).min(curr_paints.len());
    for ord in 0..n {
        arena[start + ord].screen = curr_paints[ord].screen;
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
        let li = layer.idx();
        let arena = &cascades.layers[li].paint_arena;
        let paints = &arena.rows;
        let shape_to_paint = &arena.shape_to_paint;
        for e in &tree.paint_anims.entries {
            if e.anim.next_wake(prev) <= now {
                let paint_idx = shape_to_paint[e.shape_idx as usize];
                if paint_idx != u32::MAX {
                    out.push(paints[paint_idx as usize].screen);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
