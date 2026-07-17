//! Per-frame damage detection. Computed in [`Ui::frame`] after
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
//! `cascades.layers[li].paint_arena.node_spans[i].len > 0`). Rowless
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

use crate::forest::Forest;
use crate::forest::tree::Tree;
use crate::forest::tree::iter::TreeItem;
use crate::forest::tree::node::NodeId;
use crate::forest::tree::node::SubtreeEnd;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::widget_id::WidgetIdMap;
use crate::ui::cascade::{Cascades, Paint, PaintArena};
use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use crate::ui::damage::snapshot::{
    NodeSnapshot, PaintSnapArena, ROW_UNMATCHED, has_order_inversion, push_screen,
};
use rustc_hash::FxHashSet;
use std::collections::hash_map::Entry;
use std::time::Duration;

pub(crate) mod region;
pub(crate) mod snapshot;

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
#[derive(Debug)]
pub(crate) struct DamageEngine {
    /// Per-pass merge budget (extra-overdraw px) used when
    /// `compute` builds the next frame's region. Defaults to
    /// [`DEFAULT_PASS_BUDGET_PX`]; override in place (e.g. from a
    /// debug-overlay slider, a TBDR backend init, or a test) before
    /// the next `Ui::post_record` runs.
    pub(crate) budget_px: f32,
    /// Last frame's snapshot, **only for widgets with paint rows last
    /// frame** (see the row invariant in the module doc).
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

    /// Retained scratch for [`build_row_extents`] — the per-row screen
    /// extents (child markers swapped for their subtree's painted
    /// extent) fed to [`emit_inverted_overlaps`]. Only filled on the
    /// rare frame a node's row order actually inverted; capacity
    /// persists so that frame allocates nothing.
    order_extents: Vec<Rect>,
    /// Retained scratch for the diff walk's parent tracking: one frame
    /// per open ancestor, `(subtree_end, WidgetId bits)`. Feeds each
    /// snapshot's [`NodeSnapshot::parent_key`].
    parent_stack: Vec<ParentFrame>,

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
            order_extents: Vec::new(),
            parent_stack: Vec::new(),
            #[cfg(any(test, feature = "internals"))]
            subtree_skips: 0,
        }
    }
}

/// One open ancestor on the diff walk's parent stack.
#[derive(Clone, Copy, Debug)]
struct ParentFrame {
    /// Pre-order index one past the ancestor's subtree — popped once
    /// the walk reaches it.
    end: u32,
    /// The ancestor's `WidgetId` bits — the `parent_key` of every node
    /// directly under it.
    key: u64,
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
    /// WindowRenderer-arranged surface rect for this frame. A degenerate
    /// zero-area surface is a caller logic error: hosts clamp physical
    /// size to ≥ 1 px and skip occluded windows before `Ui::frame`
    /// runs, and `DamageRegion::collapse_from` asserts on it — the one
    /// site that divides by surface area — rather than degrading
    /// silently.
    pub(crate) surface: Rect,
    pub(crate) prev_time: Option<Duration>,
    pub(crate) now: Duration,
}

impl std::fmt::Debug for DamageInput<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DamageInput")
            .field("surface", &self.surface)
            .field("prev_time", &self.prev_time)
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

/// Coverage fraction above which [`Damage::new`] stops tracking partial damage
/// and collapses straight to [`Damage::Full`]: once this much of the surface has
/// changed, the per-node filter + per-pass scissor + `LoadOp::Load` + backbuffer
/// copy bookkeeping costs more than just clearing and redrawing everything.
/// Checked against the region's sealed [`DamageRegion::coverage`]. (The
/// renderer's `DirectAdaptive` strategy applies its own, lower promote threshold
/// to the *Partial* range below this line — `DIRECT_PROMOTE_COVERAGE` in
/// `window_renderer` — but that's a present-path GPU-cost call kept out of this
/// damage-tracking one.)
///
/// The previous 0.5 was tuned for the single-rect-union accumulator, where two
/// unrelated tiny corners would blow the union to ~100 % and trip it despite
/// < 1 % of pixels actually changing. The multi-rect region keeps disjoint
/// corners disjoint at the data-structure level, so the threshold applies to the
/// *sum* of per-rect areas — corner-pair pathologies stay well below 0.7.
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

    /// Classify a region (already sealed against its surface by
    /// [`DamageRegion::collapse_from`]) into the frame's paint decision. Pure
    /// dispatch on the precomputed `coverage` — no surface needed here; the
    /// degenerate-surface check lives at the seal site.
    ///
    /// [`DamageRegion::collapse_from`]: crate::ui::damage::region::DamageRegion::collapse_from
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
        // frame wasn't acked, or first frame). Drop the per-widget
        // snapshot map here — owning the pairing keeps a caller from
        // passing `force_full` without the invalidation and corrupting
        // the next incremental diff with stale spans — then run the
        // full diff pass to repopulate it for next frame, just return
        // `Damage::Full` instead of the filtered region.
        if force_full {
            self.invalidate_prev();
        }
        #[cfg(any(test, feature = "internals"))]
        {
            self.dirty.clear();
            self.subtree_skips = 0;
        }

        // Pass 1: every damage source pushes its contributions into
        // `self.raw_rects` without applying the merge or budget
        // policy. Sources: structural diff (added / hash-changed /
        // removed widget), paint-order inversions, predamaged anim
        // rects, and the `removed`-set eviction tail. Pass 2 collapses
        // the buffer into the bounded region.
        self.raw_rects.clear();

        // Alias each mutated field once so the diff body can name
        // them independently — Entry holds the borrow on `prev` only,
        // leaving `arena` / `raw_rects` free.
        let prev_map = &mut self.prev;
        let arena = &mut self.arena;
        let raw_rects = &mut self.raw_rects;
        let order_extents = &mut self.order_extents;
        let parent_stack = &mut self.parent_stack;

        #[cfg(any(test, feature = "internals"))]
        let dirty_out = &mut self.dirty;
        #[cfg(any(test, feature = "internals"))]
        let subtree_skips_out = &mut self.subtree_skips;

        for (layer, tree) in forest.trees.iter_paint_order() {
            let layer_cascades = &cascades.layers[layer];
            let cascade_inputs = layer_cascades.cascade_inputs.as_slice();
            let node_hashes = tree.rollups.node.as_slice();
            let subtree_hashes = tree.rollups.subtree.as_slice();
            let n = tree.records.len();
            let widget_ids = tree.records.widget_id();
            let subtree_end = tree.records.subtree_end();
            let layer_paints = &layer_cascades.paint_arena.rows;
            let layer_node_paints = &layer_cascades.paint_arena.node_spans;
            let subtree_extents = layer_cascades.subtree_paint_rects.as_slice();
            parent_stack.clear();
            let mut i = 0;
            while i < n {
                while parent_stack.last().is_some_and(|f| i as u32 >= f.end) {
                    parent_stack.pop();
                }
                // Roots key on the layer discriminant, so a subtree
                // migrating between layers can't read as "unchanged".
                let parent_key = parent_stack.last().map_or(layer as u64, |f| f.key);
                let node_span = layer_node_paints[i];
                let curr_paints_slice = &layer_paints[node_span.range()];
                let curr_node_hash = node_hashes[i];
                let curr_subtree_hash = subtree_hashes[i];
                let curr_cascade_input = cascade_inputs[i];
                // This node's next-frame snapshot — every field but
                // `paint_span` is fixed per node, so the arms differ
                // only in which span they pass.
                let make_snapshot = |paint_span| NodeSnapshot {
                    paint_span,
                    hash: curr_node_hash,
                    subtree_hash: curr_subtree_hash,
                    cascade_input: curr_cascade_input,
                    parent_key,
                };
                let advance = match prev_map.entry(widget_ids[i]) {
                    // Skip the snapshot insert for a new *childless* node
                    // that paints nothing or paints entirely off-surface.
                    // `union_screens` is the former `Cascade.paint_rect`,
                    // recomputed here on the cold (Vacant) path rather than
                    // stored per node. A node with children always inserts
                    // — its child-marker rows track paint order, and its
                    // children can paint on-surface (canvas overhang) even
                    // when its own rows don't. The off-surface half of the
                    // skip is repaid by tier 1.5's insert leg the frame a
                    // move puts the rows on-surface (see the module doc's
                    // row-invariant note).
                    Entry::Vacant(_)
                        if subtree_end[i].end() as usize == i + 1
                            && !paints_on_surface(curr_paints_slice, surface) =>
                    {
                        1
                    }
                    Entry::Vacant(e) => {
                        let paint_span = arena.append(curr_paints_slice);
                        // On a force-full frame every node lands in this
                        // arm (the map was invalidated at entry) and the
                        // early return below discards the region — skip
                        // the pushes so a resize storm does no rect work
                        // and `raw_rects`' retained capacity tracks real
                        // incremental frames, not the whole tree.
                        if !force_full {
                            push_screens(raw_rects, curr_paints_slice);
                        }
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
                            && e.get().cascade_input == curr_cascade_input
                            && e.get().parent_key == parent_key =>
                    {
                        let span = (subtree_end[i].end() as usize) - i;
                        #[cfg(any(test, feature = "internals"))]
                        if span > 1 {
                            *subtree_skips_out += 1;
                        }
                        span
                    }
                    // Tier 1.5 — moved/reshaped subtree. Authoring is
                    // identical (`subtree_hash` matches ⇒ same widgets,
                    // same rows, same row hashes by induction) but
                    // `cascade_input` changed: ancestor state
                    // (transform/clip/visibility/disabled) or this
                    // node's own arranged rect moved — a scroll tick, a
                    // pan, a sibling-shift. Only the rows' *screens*
                    // differ, so damage is exactly "everything the
                    // subtree painted before ∪ everything it paints
                    // now" — two extent rects instead of the per-row
                    // hash-matcher's 2-rects-per-row flood (which made
                    // `collapse_from` + the matcher ~18% of a scrolling
                    // frame). Snapshots still need their screens +
                    // `cascade_input` refreshed for next frame's
                    // baseline; that bulk refresh happens after the
                    // match (it needs free access to `prev_map`, which
                    // the `Entry` borrow holds here) — see the
                    // `MOVED_SUBTREE` block below.
                    Entry::Occupied(e)
                        if e.get().subtree_hash == curr_subtree_hash
                            && e.get().parent_key == parent_key =>
                    {
                        MOVED_SUBTREE
                    }
                    // Tier 2 — node's own authoring + cascade state
                    // unchanged but `subtree_hash` differs, so a descendant
                    // changed. Own paints are identical (`hash` +
                    // `cascade_input` equal ⇒ identical screens), so the
                    // arena slots stay correct; just refresh the rollup and
                    // descend.
                    Entry::Occupied(mut e)
                        if e.get().hash == curr_node_hash
                            && e.get().cascade_input == curr_cascade_input
                            && e.get().parent_key == parent_key =>
                    {
                        e.get_mut().subtree_hash = curr_subtree_hash;
                        1
                    }
                    Entry::Occupied(mut e) if !curr_paints_slice.is_empty() => {
                        let prev = *e.get();
                        let leg =
                            arena.diff_changed_leg(raw_rects, prev.paint_span, curr_paints_slice);
                        // Order check — exact-matched rows emitted no content
                        // damage, but pairs whose relative paint order
                        // inverted (a raised node, a shape crossing a child
                        // boundary, coincident shapes swapping) still flip
                        // their overlap's pixels. `matched_pos` is only
                        // populated on the slow path — the fast path
                        // (`geometry_unchanged`) is order-identical by
                        // construction. Moved/added rows already pushed
                        // their full rects, which cover any overlap they
                        // sit in, so only exact pairs participate.
                        if !leg.geometry_unchanged && has_order_inversion(&arena.matched_pos) {
                            build_row_extents(
                                NodeId(i as u32),
                                tree,
                                &layer_cascades.paint_arena,
                                order_extents,
                            );
                            emit_inverted_overlaps(&arena.matched_pos, order_extents, raw_rects);
                        }
                        // A `cascade_input` change (ancestor disable,
                        // clip-saturated pan, visibility toggle) alters
                        // pixels of rows the per-shape diff matched
                        // exactly and emitted nothing for — a hidden
                        // node's untouched shapes must still clear. So
                        // the union repaints on any `cascade_input`
                        // flip, INCLUDING frames where some row also
                        // changed (`!geometry_unchanged`): gating on
                        // geometry left the exact-matched rows undamaged
                        // when a visibility flip landed on the same
                        // frame as a mid-tween shape. A pure `node_hash`
                        // flip with unchanged `cascade_input` means the
                        // authoring stream differed without touching own
                        // pixels — most commonly a child added/removed
                        // (the per-child marker folded into `node_hash`
                        // by `compute_rollups`), already covered by the
                        // subtree/eviction diff. Repainting the union
                        // there spuriously re-damages every direct shape
                        // — e.g. all canvas connections when an
                        // unrelated node is deleted.
                        if prev.cascade_input != curr_cascade_input
                            && let Some(u) = union_screens(curr_paints_slice)
                        {
                            raw_rects.push(u);
                        }
                        // Reparent / layer move at otherwise-identical
                        // content: compositing order against outside
                        // overlappers flipped, which no hash captures.
                        // The whole subtree moved together, so damage
                        // its current painted extent — descendants keep
                        // their tier-1 skip (their snapshots are intact
                        // and this push already covers them).
                        if prev.parent_key != parent_key
                            && let Some(u) = subtree_paint_extent(
                                NodeId(i as u32),
                                subtree_end,
                                &layer_cascades.paint_arena,
                            )
                        {
                            raw_rects.push(u);
                        }
                        *e.get_mut() = make_snapshot(leg.span);
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(NodeId(i as u32));
                        1
                    }
                    Entry::Occupied(e) => {
                        // Rows → rowless transition: push
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
                // Tier 1.5 body — runs outside the match so it can
                // freely probe `prev_map` for every subtree node (the
                // `Entry` above held the map borrow). Pushes the two
                // extent rects, then refreshes each descendant's
                // snapshot in place: same-count row copy (equal
                // `subtree_hash` pins the row count — `copy_from_slice`
                // length-asserts it) plus the new `cascade_input`.
                // `hash`/`subtree_hash`/`parent_key` are unchanged by
                // the same induction, and `paint_span` is reused, so no
                // arena append/orphan churn. Entry-less nodes (skipped
                // by the Vacant-arm off-surface filter back when they
                // painted nothing visible) get their snapshot inserted
                // the moment a move puts their rows on-surface —
                // without that, the node's pixels stay invisible to
                // later prev-extent folds and to the removed-widget
                // eviction tail (stale pixels on the next move /
                // removal — the second-move and removal legs of
                // `offscreen_node_scrolling_into_view_is_covered_and_stays_sound`
                // pin this).
                let advance = if advance == MOVED_SUBTREE {
                    let end = subtree_end[i].end() as usize;
                    let mut prev_extent: Option<Rect> = None;
                    // Mini parent stack over the jump so inserted
                    // snapshots carry the same `parent_key` the
                    // per-node walk would have stamped. Rides the tail
                    // of the outer `parent_stack`; truncated below.
                    let jump_base = parent_stack.len();
                    for j in i..end {
                        while parent_stack.len() > jump_base
                            && parent_stack.last().is_some_and(|f| j as u32 >= f.end)
                        {
                            parent_stack.pop();
                        }
                        // At `j == i` the stack top is still `i`'s own
                        // parent (or empty at a root), so this reads
                        // the outer `parent_key` — one expression
                        // serves the whole range.
                        let j_parent_key = parent_stack.last().map_or(parent_key, |f| f.key);
                        let j_end = subtree_end[j].end();
                        if j_end as usize > j + 1 {
                            parent_stack.push(ParentFrame {
                                end: j_end,
                                key: widget_ids[j].0,
                            });
                        }
                        let span = layer_node_paints[j];
                        if span.len == 0 {
                            continue;
                        }
                        match prev_map.get_mut(&widget_ids[j]) {
                            Some(snap) => {
                                if let Some(u) =
                                    union_screens(&arena.snaps[snap.paint_span.range()])
                                {
                                    prev_extent = Some(prev_extent.map_or(u, |a| a.union(u)));
                                }
                                arena.snaps[snap.paint_span.range()]
                                    .copy_from_slice(&layer_paints[span.range()]);
                                snap.cascade_input = cascade_inputs[j];
                            }
                            // Off-surface at its last per-node visit ⇒
                            // it painted nothing, so there are no prev
                            // pixels to fold; its current pixels are
                            // inside the curr-extent push either way.
                            // Insert the snapshot the Vacant arm would
                            // have once the rows land on-surface, so
                            // the node is tracked from its first
                            // visible frame.
                            None => {
                                let curr = &layer_paints[span.range()];
                                if !paints_on_surface(curr, surface) {
                                    continue;
                                }
                                let paint_span = arena.append(curr);
                                prev_map.insert(
                                    widget_ids[j],
                                    NodeSnapshot {
                                        paint_span,
                                        hash: node_hashes[j],
                                        subtree_hash: subtree_hashes[j],
                                        cascade_input: cascade_inputs[j],
                                        parent_key: j_parent_key,
                                    },
                                );
                            }
                        }
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(NodeId(j as u32));
                    }
                    parent_stack.truncate(jump_base);
                    if let Some(u) = prev_extent {
                        raw_rects.push(u);
                    }
                    // Rolled-up curr extent from the cascade — already
                    // `Rect::ZERO`-seeded for invisible subtrees, so a
                    // hide transition damages only the prev pixels.
                    let curr_extent = subtree_extents[i];
                    if !curr_extent.is_paint_empty() {
                        raw_rects.push(curr_extent);
                    }
                    end - i
                } else {
                    advance
                };
                // Descending into children (advance == 1 on a
                // container) opens a parent frame; subtree-skips jump
                // past their descendants, so nothing to push there.
                if advance == 1 {
                    let end = subtree_end[i].end();
                    if end as usize > i + 1 {
                        parent_stack.push(ParentFrame {
                            end,
                            key: widget_ids[i].0,
                        });
                    }
                }
                i += advance;
            }
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
        push_screen(out, p.screen);
    }
}

/// Screen-space union of a node's pixel-producing paint rows — the
/// node's own paint extent, formerly stored as `Cascade.paint_rect`.
/// The cascade no longer caches it; the damage diff recomputes it here
/// on its cold paths (the Vacant surface-cull and the tier-3
/// cascade-state union push) from the same `paint_arena` slice those
/// arms already touch. Paint-empty rows (child markers, clipped-away
/// shapes) are skipped — folding their zero boxes in would bias the
/// union toward the origin / clip edge. `None` when no row produces
/// pixels.
#[inline]
fn union_screens(paints: &[Paint]) -> Option<Rect> {
    paints
        .iter()
        .map(|p| p.screen)
        .filter(|s| !s.is_paint_empty())
        .reduce(|acc, s| acc.union(s))
}

/// True when any of the node's paint rows produces visible pixels on
/// `surface`. One predicate, two coupled sites: the Vacant arm skips
/// the snapshot insert for a childless node where this is false, and
/// tier 1.5's insert leg repays that skip the frame it turns true —
/// they must agree or painting-but-untracked nodes reappear.
#[inline]
fn paints_on_surface(paints: &[Paint], surface: Rect) -> bool {
    paints
        .iter()
        .any(|paint| !paint.screen.is_paint_empty() && paint.screen.intersects(surface))
}

/// `advance` sentinel returned by the diff's tier-1.5 match arm
/// (moved/reshaped subtree). The refresh body runs *after* the match —
/// it needs `prev_map` access the `Entry` borrow forbids — and this
/// value routes to it. Real advances are bounded by the tree size, so
/// the sentinel can't collide.
const MOVED_SUBTREE: usize = usize::MAX;

/// Screen-space extent per row of `node`'s paint span, in row order:
/// chrome and direct shapes keep their own `Paint.screen`; a child
/// marker's zero rect is swapped for [`child_paint_extent`] — the
/// pixels that actually move when the child's paint order flips. Only
/// built on the inversion path — child extents walk the whole child
/// subtree's rows.
///
/// Rows are 1:1 with chrome + the node's `TreeItems` stream (the
/// cascade emits them from the same walk), so the two cursors advance
/// in lockstep.
fn build_row_extents(node: NodeId, tree: &Tree, arena: &PaintArena, out: &mut Vec<Rect>) {
    let node_span = arena.node_spans[node.idx()];
    let subtree_end = tree.records.subtree_end();
    out.clear();
    if tree.chrome(node).is_some() {
        out.push(arena.rows[node_span.start as usize].screen);
    }
    for item in tree.tree_items(node) {
        out.push(match item {
            TreeItem::ShapeRecord(..) => arena.rows[node_span.start as usize + out.len()].screen,
            TreeItem::Child(c) => {
                subtree_paint_extent(c.id, subtree_end, arena).unwrap_or(Rect::ZERO)
            }
        });
    }
    debug_assert_eq!(
        out.len(),
        node_span.len as usize,
        "row extents out of sync with the owner's paint span",
    );
}

/// Push the extent intersection of every exact-matched row pair whose
/// relative paint order inverted since last frame. `extents[j]` is the
/// curr row's screen extent (a [`build_row_extents`] slot). O(rows²)
/// pair enumeration, reached only behind a [`has_order_inversion`]
/// gate on the rare frame an order actually flipped. Rows that merely
/// shifted because a sibling was added or removed keep their relative
/// order and contribute nothing. `push_screen` drops degenerate
/// results — a zero-size extent pinned strictly inside a sibling DOES
/// pass `intersects` (all four strict compares hold), and a sub-EPS
/// overlap sliver paints nothing; neither earns a merge slot.
fn emit_inverted_overlaps(matched_pos: &[u32], extents: &[Rect], out: &mut Vec<Rect>) {
    for j2 in 1..matched_pos.len() {
        let p2 = matched_pos[j2];
        if p2 == ROW_UNMATCHED {
            continue;
        }
        for (j1, &p1) in matched_pos.iter().enumerate().take(j2) {
            if p1 == ROW_UNMATCHED || p1 < p2 {
                continue;
            }
            push_screen(out, extents[j1].intersect(extents[j2]));
        }
    }
}

/// Screen-space painted extent of `node`'s whole subtree — the union of
/// every paint row in `[node, subtree_end)`. Built from the per-shape
/// `Paint.screen` rects (already transformed + clipped) rather than
/// `Cascades::subtree_paint_rects` so a non-painting descendant can't
/// bias the extent. `None` when the subtree paints nothing. Used for a
/// child marker's row extent in [`build_row_extents`] and for the
/// reparent/layer-move damage push in the diff walk.
///
/// The cascade visits nodes in pre-order with a monotone arena cursor
/// and stamps every node's `node_spans` slot (empty spans still carry
/// the cursor as `start`), so a subtree's rows are one contiguous run:
/// from the node's own `start` to the `start` of the first node past
/// the subtree (or the arena's end). One linear fold, no per-node span
/// hops.
fn subtree_paint_extent(
    node: NodeId,
    subtree_end: &[SubtreeEnd],
    arena: &PaintArena,
) -> Option<Rect> {
    let end = subtree_end[node.idx()].end() as usize;
    let start_row = arena.node_spans[node.idx()].start as usize;
    let end_row = match arena.node_spans.get(end) {
        Some(next) => next.start as usize,
        None => arena.rows.len(),
    };
    union_screens(&arena.rows[start_row..end_row])
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
    for (layer, tree) in forest.trees.iter_paint_order() {
        let arena = &cascades.layers[layer].paint_arena;
        let paints = &arena.rows;
        let node_spans = &arena.node_spans;
        for e in &tree.paint_anims.entries {
            if e.anim.next_wake(prev) > now {
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
        union_screens(&self.arena.snaps[snap.paint_span.range()])
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    use crate::forest::Forest;
    use crate::ui::damage::DamageEngine;

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
