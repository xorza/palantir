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
//! **Paint order.** Child markers put the shape/child interleave into
//! each node's row span, and `compute_hashes` folds child identity
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
use crate::forest::node::SubtreeEnd;
use crate::forest::rollups::{CascadeInputHash, NodeHash};
use crate::forest::seen_ids::WidgetIdMap;
use crate::forest::tree::iter::TreeItem;
use crate::forest::tree::{NodeId, Tree};
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::{Cascades, Paint, PaintArena};
use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use rustc_hash::FxHashSet;
use std::collections::hash_map::Entry;
use std::time::Duration;

pub mod region;

/// Per-widget snapshot held in [`DamageEngine::prev`], keyed by stable
/// [`WidgetId`]. Only widgets with paint rows last frame have an entry
/// — rowless nodes (e.g. a popup's childless invisible click-eater)
/// are skipped on insert, so their full-surface rect can't trip the
/// full-repaint coverage threshold on add or remove.
///
/// **Storage shape.** Per-paint snapshots don't live inline here —
/// they live in [`DamageEngine::arena`], a single contiguous
/// arena shared by every widget, and this struct just holds a `Span`
/// into it. Each row is chrome (row 0 when present), one direct
/// shape, or a child marker, mirroring `LayerCascades::paint_arena`.
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
    /// when present, then shapes + child markers). Never empty — the
    /// row invariant means rowless nodes don't get an entry in `prev`
    /// at all.
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
    /// Paint-order position: the immediate parent's `WidgetId` bits,
    /// or the layer discriminant for a root. A widget reparented (or
    /// moved to another layer) at an identical rect with identical
    /// content keeps `hash`, `subtree_hash`, AND `cascade_input`
    /// (which folds ancestor *state*, not identity) — yet its
    /// compositing order against outside overlappers flipped, so the
    /// skip tiers must not treat it as unchanged.
    pub(crate) parent_key: u64,
}

/// Per-widget paint snapshots packed contiguously, plus the
/// scratch buffer + orphan counter that drive double-buffered
/// compaction. Each [`NodeSnapshot::paint_span`] is a slice into
/// `snaps`; chrome lives at row 0 of the owner's span when present,
/// followed by direct shapes and child markers in record order.
///
/// Lifecycle: append-only on count-change paths (new span at end,
/// old slice orphaned); in-place on same-count refreshes;
/// [`Self::maybe_compact`] reseats live spans into `scratch` once
/// orphans exceed the threshold, then swaps. Retained capacity —
/// steady-state alloc-free even under paint-count churn.
#[derive(Debug, Default)]
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
    /// Pass-1 exact-match position map: for each curr paint, the prev
    /// row it paired with (`ROW_UNMATCHED` when pass 1 didn't pair
    /// it). Feeds the within-node order-inversion check — an exact
    /// pair emits no content damage, but two of them swapping paint
    /// order still flips their overlap's pixels. Capacity retained.
    matched_pos: Vec<u32>,
    /// Sort-merge scratch for the content-keyed passes: `(key, row
    /// index)` pairs for the unclaimed prev side / the unmatched curr
    /// side. Sorting + merging both sides replaces the old restart-
    /// from-zero first-fit scan, bounding the all-rows-shifted case
    /// (one shape inserted at the front of a big node) at
    /// O(n log n) instead of O(n²). Capacity retained.
    prev_keyed: Vec<(PaintKey, u32)>,
    curr_keyed: Vec<(PaintKey, u32)>,
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

/// Result of [`PaintSnapArena::diff_changed_leg`].
#[derive(Debug)]
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
    /// a single-pass matcher would emit. Both passes run as sorted
    /// merges over `(PaintKey, index)` scratch — ascending-index
    /// pairing within equal-key runs, the same claims the first-fit
    /// scan produced, at O(n log n) instead of O(n²) when every row
    /// shifted (one shape inserted at the front of a big node).
    ///
    /// Sub-pixel float wobble on `Paint.screen` (composer's pixel
    /// snapping runs downstream) makes strict `==` brittle; the
    /// hash-only fallback recovers the move signal without losing the
    /// exact-match optimisation.
    ///
    /// **Order check** — exact pairs emit no content damage, but two of
    /// them swapping paint order still flips their overlap's pixels
    /// (two coincident wires trading which is on top, a raised node,
    /// a shape crossing a child boundary — child markers make all of
    /// these row reorders). This leg only *records* the pairing: on
    /// the slow path `matched_pos` is left populated for the caller,
    /// who runs [`has_order_inversion`] and emits each inverted pair's
    /// extent overlap — child-marker extents need tree context this
    /// arena doesn't hold.
    ///
    /// Pass 1's positional pre-pass pairs in-place rows in O(n); only
    /// the leftovers pay the keyed sort + merge. The retained
    /// `prev_matched` / `matched_pos` / `prev_keyed` / `curr_keyed`
    /// scratch keeps every pass alloc-free across frames; empty
    /// leftovers (every shape paired positionally) make both merges
    /// trivially skip. The slow path spills `curr_paints` to the
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
        // and write the scratch bitmap + keyed-index lists simultaneously.
        let Self {
            snaps,
            prev_matched,
            matched_pos,
            prev_keyed,
            curr_keyed,
            ..
        } = self;
        let prev_slice = &snaps[prev_start..prev_start + prev_len];

        prev_matched.clear();
        prev_matched.resize(prev_len, false);
        matched_pos.clear();
        matched_pos.resize(curr_paints.len(), ROW_UNMATCHED);

        // Pass 1 — exact (screen, hash) pairs. No damage. A positional
        // pre-pass claims same-index matches first: the dominant churn
        // shape (one shape changed, the rest in place — every wire of a
        // dragged canvas node) pairs in O(n). Identical rows are
        // interchangeable, so which duplicate pairs up doesn't matter.
        for (j, (&c, &p)) in curr_paints.iter().zip(prev_slice).enumerate() {
            if p == c {
                prev_matched[j] = true;
                matched_pos[j] = j as u32;
            }
        }
        // Key the leftovers once for both merge passes below. Sorting
        // by (key, row index) makes each merge claim ascending indices
        // on both sides — the same pairing the old first-fit scan
        // produced.
        prev_keyed.clear();
        curr_keyed.clear();
        for (i, p) in prev_slice.iter().enumerate() {
            if !prev_matched[i] {
                prev_keyed.push((PaintKey::of(p), i as u32));
            }
        }
        for (j, c) in curr_paints.iter().enumerate() {
            if matched_pos[j] == ROW_UNMATCHED {
                curr_keyed.push((PaintKey::of(c), j as u32));
            }
        }
        prev_keyed.sort_unstable();
        curr_keyed.sort_unstable();

        // Pass 1b — exact pairs anywhere in the span, merged over the
        // sorted keys.
        let (mut pi, mut ci) = (0, 0);
        while pi < prev_keyed.len() && ci < curr_keyed.len() {
            let (pk, prow) = prev_keyed[pi];
            let (ck, crow) = curr_keyed[ci];
            match pk.cmp(&ck) {
                std::cmp::Ordering::Less => pi += 1,
                std::cmp::Ordering::Greater => ci += 1,
                std::cmp::Ordering::Equal => {
                    // Key-equal ⇒ bit-equal (modulo -0.0), but NaN
                    // screens are never `==` — confirm before pairing.
                    if prev_slice[prow as usize] == curr_paints[crow as usize] {
                        prev_matched[prow as usize] = true;
                        matched_pos[crow as usize] = prow;
                        ci += 1;
                    }
                    pi += 1;
                }
            }
        }

        // Pass 2 — hash-only pairs surface as moves; unmatched curr
        // surfaces as adds. `PaintKey` orders hash-major, so the same
        // sorted buffers merge by hash alone — no re-sort. Child
        // markers can't reach the move leg's pushes with anything
        // visible — their screens are zero (paint-empty), so the
        // pushes below skip them; an added/removed child's pixels
        // are damaged by its own nodes' Vacant/evict arms.
        let mut pi = 0;
        for &(ck, crow) in curr_keyed.iter() {
            if matched_pos[crow as usize] != ROW_UNMATCHED {
                continue;
            }
            while pi < prev_keyed.len() {
                let (pk, prow) = prev_keyed[pi];
                if prev_matched[prow as usize] || pk.hash < ck.hash {
                    pi += 1;
                } else {
                    break;
                }
            }
            match prev_keyed.get(pi) {
                Some(&(pk, prow)) if pk.hash == ck.hash => {
                    push_screen(out, prev_slice[prow as usize].screen);
                    push_screen(out, curr_paints[crow as usize].screen);
                    prev_matched[prow as usize] = true;
                    pi += 1;
                }
                _ => push_screen(out, curr_paints[crow as usize].screen),
            }
        }
        // Remaining prev paints — removals.
        for (i, &p) in prev_slice.iter().enumerate() {
            if !prev_matched[i] {
                push_screen(out, p.screen);
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
                // Row invariant: every entry in `prev` covers at least
                // one Paint row (chrome at row 0 OR ≥1 shape / child
                // marker). A zero-len snap would have a stale `start`
                // after the swap below — assert rather than silently
                // skip.
                assert!(
                    snap.paint_span.len > 0,
                    "PaintSnapArena::compact: prev entry for {wid:?} has zero-len paint_span, \
                     violating the row invariant",
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

        for (layer, tree) in forest.iter_paint_order() {
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
                    // when its own rows don't.
                    Entry::Vacant(_)
                        if subtree_end[i].end() as usize == i + 1
                            && !union_screens(curr_paints_slice)
                                .is_some_and(|u| u.intersects(surface)) =>
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
                        // by `compute_hashes`), already covered by the
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
                // arena append/orphan churn.
                let advance = if advance == MOVED_SUBTREE {
                    let end = subtree_end[i].end() as usize;
                    let mut prev_extent: Option<Rect> = None;
                    for j in i..end {
                        let span = layer_node_paints[j];
                        if span.len == 0 {
                            continue;
                        }
                        // No entry ⇒ the node was skipped by the
                        // Vacant-arm off-surface filter last visit; it
                        // painted nothing then, and its current pixels
                        // are inside the curr-extent push.
                        let Some(snap) = prev_map.get_mut(&widget_ids[j]) else {
                            continue;
                        };
                        if let Some(u) = union_screens(&arena.snaps[snap.paint_span.range()]) {
                            prev_extent = Some(prev_extent.map_or(u, |a| a.union(u)));
                        }
                        arena.snaps[snap.paint_span.range()]
                            .copy_from_slice(&layer_paints[span.range()]);
                        snap.cascade_input = cascade_inputs[j];
                        #[cfg(any(test, feature = "internals"))]
                        dirty_out.push(NodeId(j as u32));
                    }
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
        // Provably a no-op today (nothing on this path orphans arena
        // entries) — runs anyway so the fast path stays self-healing
        // if predamage ever starts mutating snapshots.
        self.arena.maybe_compact(input.forest, &mut self.prev);
        self.finish_region(input.surface)
    }
}

/// Push one screen rect into the raw-rect buffer, dropping
/// paint-empty rects — child markers (always zero) and fully
/// clipped-away shapes produce no pixels, so they have nothing to
/// clear or repaint.
#[inline]
fn push_screen(out: &mut Vec<Rect>, screen: Rect) {
    if !screen.is_paint_empty() {
        out.push(screen);
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

/// `matched_pos` sentinel for a curr row with no exact match in the
/// prev span (moved / added / content-changed — the content diff
/// damages those over their full rects).
const ROW_UNMATCHED: u32 = u32::MAX;

/// `advance` sentinel returned by the diff's tier-1.5 match arm
/// (moved/reshaped subtree). The refresh body runs *after* the match —
/// it needs `prev_map` access the `Entry` borrow forbids — and this
/// value routes to it. Real advances are bounded by the tree size, so
/// the sentinel can't collide.
const MOVED_SUBTREE: usize = usize::MAX;

/// Sort key for the content-keyed matcher: hash-major (so one sorted
/// order serves both the exact pass and the hash-only move pass),
/// then the screen rect's bit pattern with `-0.0` normalized to
/// `+0.0` (the two compare equal under `Paint ==` and must land in
/// one run). Key-equal rows still confirm with a real `Paint ==`
/// before pairing, so NaN screens — key-equal but never `==` —
/// can't false-pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PaintKey {
    hash: u64,
    screen_bits: [u32; 4],
}

impl PaintKey {
    fn of(p: &Paint) -> PaintKey {
        // `f + 0.0` folds -0.0 onto +0.0 and leaves every other value
        // (NaN included) bit-stable.
        let n = |f: f32| (f + 0.0).to_bits();
        PaintKey {
            hash: p.hash.0,
            screen_bits: [
                n(p.screen.min.x),
                n(p.screen.min.y),
                n(p.screen.size.w),
                n(p.screen.size.h),
            ],
        }
    }
}

/// True when some pair of matched rows inverted its relative order —
/// i.e. the matched prev positions aren't non-decreasing in curr order.
/// O(n) gate in front of the quadratic pair enumeration. Equal
/// adjacent positions can't occur (each prev row is claimed at most
/// once), so allow-equal `is_sorted` is exact.
fn has_order_inversion(matched_pos: &[u32]) -> bool {
    !matched_pos
        .iter()
        .filter(|&&pos| pos != ROW_UNMATCHED)
        .is_sorted()
}

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
    assert_eq!(
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
    for (layer, tree) in forest.iter_paint_order() {
        let arena = &cascades.layers[layer].paint_arena;
        let paints = &arena.rows;
        let node_spans = &arena.node_spans;
        for e in &tree.paint_anims.entries {
            if e.anim.next_wake(prev) > now {
                continue;
            }
            // Map the shape to its paint slot inside the owner's
            // `node_span`. Chrome (when present) sits at row 0, then
            // one row per `TreeItems` item — direct shapes and child
            // markers alike, in the same stream `compute_paint_rect`
            // emitted from — so the row index is the shape's position
            // in that stream. `shape_idx - shape_span.start` would be
            // wrong here: the span covers the whole subtree, so a
            // shape-bearing child recorded before the animated shape
            // (Scroll's bars-after-child pattern) would shift the
            // arithmetic onto a different row.
            let node = NodeId(e.node_idx);
            let ordinal = tree
                .tree_items(node)
                .position(
                    |item| matches!(item, TreeItem::ShapeRecord(idx, _) if idx == e.shape_idx),
                )
                .expect("paint-anim shape_idx is a direct shape of its owner")
                as u32;
            let chrome_offset = u32::from(tree.chrome(node).is_some());
            let node_span = node_spans[e.node_idx as usize];
            let want = chrome_offset + ordinal;
            // `compute_paint_rect` emits one row per direct shape for
            // every node (invisible included), so the slot must exist.
            assert!(
                want < node_span.len,
                "paint-anim row {want} out of the owner's {} paint rows",
                node_span.len,
            );
            out.push(paints[(node_span.start + want) as usize].screen);
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
