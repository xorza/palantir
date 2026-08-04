//! Cross-frame node and paint snapshot storage used by the damage diff.

use crate::common::content_hash::ContentHash;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetIdMap;
use crate::scene::cascade::CascadeInputHash;
use crate::scene::cascade::paint::Paint;
use crate::scene::damage::push_screen;
use crate::scene::forest::Forest;
use std::cmp::Ordering;

/// Minimum [`PaintSnapArena::snaps`] length before [`PaintSnapArena::maybe_compact`]
/// considers running. Below this the arena is small enough that the
/// reseat walk costs more than the orphaned-slot memory it would
/// reclaim — capacity is `Vec`-amortised and these entries stay hot
/// in cache. Empirically tuned against `src/scene/damage/bench.rs`; change
/// with a benchmark on the damage-merge fixture.
const COMPACT_MIN_TOTAL: u32 = 256;

/// Orphan-ratio threshold (in 1/4 units) above which compaction
/// triggers — `orphaned * 4 >= total * COMPACT_ORPHAN_RATIO_NUM` is
/// the predicate. `3/4 = 75%` orphaned means three quarters of the
/// arena is dead bytes before a reseat pays off; lower values cause
/// thrash on churn-heavy frames. Tuned the same way as
/// [`COMPACT_MIN_TOTAL`] — change it with a benchmark, not by feel.
const COMPACT_ORPHAN_RATIO_NUM: u32 = 3;

/// `matched_pos` sentinel for a curr row with no exact match in the
/// prev span (moved / added / content-changed — the content diff
/// damages those over their full rects).
pub(super) const ROW_UNMATCHED: u32 = u32::MAX;

/// Per-widget snapshot held in [`crate::scene::damage::DamageEngine::prev`], keyed by stable
/// [`WidgetId`](crate::primitives::widget_id::WidgetId). Only widgets with
/// paint rows last frame have an entry
/// — rowless nodes (e.g. a popup's childless invisible click-eater)
/// are skipped on insert, so their full-surface rect can't trip the
/// full-repaint coverage threshold on add or remove.
///
/// **Storage shape.** Per-paint snapshots don't live inline here —
/// they live in [`crate::scene::damage::DamageEngine::arena`], a single contiguous
/// arena shared by every widget, and this struct just holds a `Span`
/// into it. Each row is chrome (row 0 when present), one direct
/// shape, or a child marker, mirroring `LayerCascade::paint_arena`.
///
/// **No cached `rect`.** The node's own paint extent — the union of its
/// `paint_arena` rows, folded by
/// [`PaintRows::union_screens`](crate::scene::cascade::paint::PaintRows::union_screens)
/// — is a pure function of `(hash, cascade_input)`:
/// every geometry input (`layout_rect`, ancestor transform/clip) lives
/// in `cascade_input` and every shape input lives in `hash`, so a
/// snapshot field would be a redundant cache of those two. The diff
/// keys the "node unchanged" fast path on `(hash, cascade_input)`
/// directly; the per-shape screen rects needed when something *did*
/// change are recovered from `paint_span`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// Slice into [`crate::scene::damage::DamageEngine::arena`] describing this
    /// widget's per-paint snapshots in record order (chrome at row 0
    /// when present, then shapes + child markers). Never empty — the
    /// row invariant means rowless nodes don't get an entry in `prev`
    /// at all.
    pub(super) paint_span: Span,
    /// Authoring hash from last frame's `Tree.rollups.node`.
    pub(crate) hash: ContentHash,
    /// Rollup hash of this node + its entire subtree from last frame's
    /// `Tree.rollups.subtree`. Pair with `cascade_input` to drive the
    /// subtree-skip fast path: if both match the current frame, every
    /// descendant is bit-identical and the per-node diff can jump to
    /// `subtree_end[i]`.
    pub(super) subtree_hash: ContentHash,
    /// Fingerprint of last frame's cascade inputs at this node (parent
    /// transform/clip/disabled/invisible + own arranged rect). See
    /// [`CascadeInputHash`].
    pub(super) cascade_input: CascadeInputHash,
    /// Paint-order position: the immediate parent's `WidgetId` bits,
    /// or the layer discriminant for a root. A widget reparented (or
    /// moved to another layer) at an identical rect with identical
    /// content keeps `hash`, `subtree_hash`, AND `cascade_input`
    /// (which folds ancestor *state*, not identity) — yet its
    /// compositing order against outside overlappers flipped, so the
    /// skip tiers must not treat it as unchanged.
    pub(super) parent_key: u64,
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
    /// The content-keyed matcher's scratch. Its own type so each phase
    /// can be a method that names the precondition it inherits.
    matcher: RowMatcher,
    /// Count of `Paint` entries in `snaps` that no live
    /// `NodeSnapshot::paint_span` points into. Drives the compaction
    /// trigger.
    pub(crate) orphaned: u32,
    /// Compaction-event counter — bumped each time [`Self::compact`]
    /// runs. Gated behind `internals` so benches can verify the path
    /// was actually exercised.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) compactions_run: u32,
}

/// Result of [`PaintSnapArena::diff_changed_leg`].
#[derive(Debug)]
pub(super) struct ChangedLeg {
    /// Span covering this frame's paints — `prev_span` reused when the
    /// row count is stable, a fresh tail span when it changes.
    pub(super) span: Span,
    /// True when every `Paint` matched bit-identically (the fast path),
    /// so the per-shape diff emitted *no* damage. Reaching the
    /// changed-paints arm at all means `hash` or `cascade_input`
    /// changed, so a `true` here means a cascade-state toggle (ancestor
    /// disabled / clip-saturated pan) altered the node's pixels without
    /// moving any shape — the caller must damage the union to repaint
    /// it.
    pub(super) geometry_unchanged: bool,
}

impl PaintSnapArena {
    /// Which prev row each curr row paired with on the last
    /// [`Self::diff_changed_leg`], for the caller's order-inversion
    /// check. `ROW_UNMATCHED` where nothing paired.
    #[inline]
    pub(super) fn matched_positions(&self) -> &[u32] {
        &self.matcher.matched_pos
    }

    /// Reset to empty — caller's next `compute` will repopulate.
    pub(super) fn clear(&mut self) {
        self.snaps.clear();
        self.orphaned = 0;
    }

    /// Append `paints` to the tail and return the covering [`Span`].
    /// Used by the Vacant-insert arm of `compute`.
    pub(super) fn append(&mut self, paints: &[Paint]) -> Span {
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
    /// trivially skip. The slow path refreshes the existing span when
    /// the row count is stable. Count changes spill `curr_paints` to
    /// the tail of `snaps` and route the prev span through
    /// [`Self::mark_orphaned`]; `maybe_compact` reclaims the tail once
    /// orphans accumulate.
    pub(super) fn diff_changed_leg(
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

        // Split-borrow: the phases read `prev` out of `snaps` while
        // writing the matcher's columns. Disjoint fields, one
        // destructure.
        let Self { snaps, matcher, .. } = self;
        let prev = &snaps[prev_start..prev_start + prev_len];

        matcher.begin(prev, curr_paints);
        matcher.claim_exact(prev, curr_paints);
        matcher.emit_moves_and_adds(out, prev, curr_paints);
        matcher.emit_removals(out, prev);

        let span = if prev_len == curr_paints.len() {
            snaps[prev_span.range()].copy_from_slice(curr_paints);
            prev_span
        } else {
            let new_start = snaps.len() as u32;
            snaps.extend_from_slice(curr_paints);
            self.mark_orphaned(prev_len as u32);
            Span::new(new_start, curr_paints.len() as u32)
        };
        ChangedLeg {
            span,
            geometry_unchanged: false,
        }
    }

    /// Mark `n` paint entries as orphaned (their owning snapshot was
    /// evicted or its span was relocated). Saturating to avoid wrap
    /// in the unlikely 4-billion-orphan edge case.
    #[inline]
    pub(super) fn mark_orphaned(&mut self, n: u32) {
        self.orphaned = self.orphaned.saturating_add(n);
    }

    /// Walk live `NodeSnapshot::paint_span`s in pre-order paint
    /// order and reseat into `scratch`, then swap.
    pub(super) fn compact(&mut self, forest: &Forest, prev: &mut WidgetIdMap<NodeSnapshot>) {
        self.scratch.clear();
        for (_layer, tree) in forest.trees.iter_paint_order() {
            for wid in tree.records.widget_id() {
                let Some(snap) = prev.get_mut(wid) else {
                    continue;
                };
                // Row invariant: every entry in `prev` covers at least
                // one Paint row (chrome at row 0 OR ≥1 shape / child
                // marker). A zero-len snap would have a stale `start`
                // after the swap below — assert rather than silently
                // skip.
                debug_assert!(
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
        self.note_compaction();
    }

    /// Bump the compaction counter. Same principle as
    /// [`DamageProbe`](crate::scene::damage::probe::DamageProbe): the gate
    /// lives in here so the call site above carries none.
    ///
    /// Kept on the arena rather than folded into `DamageProbe` because the
    /// lifetimes differ — that probe's counters reset every pass, while
    /// this one accumulates for the life of the arena and the bench reads
    /// it as a delta across many passes.
    #[inline]
    fn note_compaction(&mut self) {
        #[cfg(any(test, feature = "internals"))]
        {
            self.compactions_run = self.compactions_run.saturating_add(1);
        }
    }

    /// Trigger compaction when the arena is large enough
    /// ([`COMPACT_MIN_TOTAL`]) and orphaned entries are ≥ 75 % of the
    /// buffer ([`COMPACT_ORPHAN_RATIO_NUM`]/4).
    pub(super) fn maybe_compact(&mut self, forest: &Forest, prev: &mut WidgetIdMap<NodeSnapshot>) {
        let total = self.snaps.len() as u32;
        if total >= COMPACT_MIN_TOTAL
            && self.orphaned.saturating_mul(4) >= total * COMPACT_ORPHAN_RATIO_NUM
        {
            self.compact(forest, prev);
        }
    }
}

/// Scratch for the content-keyed row matcher, and the phases that fill
/// it.
///
/// One type rather than four loose `Vec`s on the arena so the phase
/// order reads as a call sequence and each phase's doc can state what
/// the previous one left behind. The sort order in particular is
/// load-bearing twice over, and stating it on [`PaintKey`] alone would
/// leave it forty lines from the merge that depends on it.
///
/// Every column keeps its capacity across frames, so a steady-state
/// content reshuffle allocates nothing.
#[derive(Debug, Default)]
struct RowMatcher {
    /// Which prev rows have been claimed by some phase.
    prev_matched: Vec<bool>,
    /// For each curr row, the prev row it paired with, or
    /// [`ROW_UNMATCHED`]. Survives the call for the caller's
    /// order-inversion check: an exact pair emits no content damage, but
    /// two of them swapping paint order still flips their overlap.
    matched_pos: Vec<u32>,
    /// `(key, row)` for the rows each side still has unclaimed, sorted.
    /// Sorting and merging replaced a restart-from-zero first-fit scan,
    /// bounding the all-rows-shifted case (one shape inserted at the
    /// front of a big node) at O(n log n) rather than O(n²).
    prev_keyed: Vec<(PaintKey, u32)>,
    curr_keyed: Vec<(PaintKey, u32)>,
}

impl RowMatcher {
    /// Phase 1 — reset, claim every same-index bit-identical pair, and
    /// key whatever is left over.
    ///
    /// The positional pre-pass is what makes the dominant churn shape
    /// cheap: one shape changed and the rest in place — every wire of a
    /// dragged canvas node — pairs in O(n) and leaves both keyed lists
    /// empty, so the merges below skip trivially. Identical rows are
    /// interchangeable, so which duplicate pairs up doesn't matter.
    ///
    /// **Leaves both keyed lists sorted by `(key, row)`.** Ascending row
    /// within an equal-key run makes the merges claim ascending indices
    /// on both sides, reproducing the first-fit scan's pairing; and
    /// `PaintKey` being hash-major is what lets
    /// [`Self::emit_moves_and_adds`] re-merge the very same buffers on
    /// hash alone without re-sorting.
    fn begin(&mut self, prev: &[Paint], curr: &[Paint]) {
        self.prev_matched.clear();
        self.prev_matched.resize(prev.len(), false);
        self.matched_pos.clear();
        self.matched_pos.resize(curr.len(), ROW_UNMATCHED);
        self.prev_keyed.clear();
        self.curr_keyed.clear();

        let shared = prev.len().min(curr.len());
        for row in 0..shared {
            let (p, c) = (prev[row], curr[row]);
            if p == c {
                self.prev_matched[row] = true;
                self.matched_pos[row] = row as u32;
            } else {
                self.prev_keyed.push((PaintKey::of(&p), row as u32));
                self.curr_keyed.push((PaintKey::of(&c), row as u32));
            }
        }
        for (offset, p) in prev[shared..].iter().enumerate() {
            self.prev_keyed
                .push((PaintKey::of(p), (shared + offset) as u32));
        }
        for (offset, c) in curr[shared..].iter().enumerate() {
            self.curr_keyed
                .push((PaintKey::of(c), (shared + offset) as u32));
        }
        self.prev_keyed.sort_unstable();
        self.curr_keyed.sort_unstable();
    }

    /// Phase 2 — exact `(screen, hash)` pairs anywhere in the span, not
    /// just at matching indices. Emits no damage: same shape, same place.
    ///
    /// Requires the sorted keyed lists [`Self::begin`] leaves.
    fn claim_exact(&mut self, prev: &[Paint], curr: &[Paint]) {
        let (mut pi, mut ci) = (0, 0);
        while pi < self.prev_keyed.len() && ci < self.curr_keyed.len() {
            let (pk, prow) = self.prev_keyed[pi];
            let (ck, crow) = self.curr_keyed[ci];
            match pk.cmp(&ck) {
                Ordering::Less => pi += 1,
                Ordering::Greater => ci += 1,
                Ordering::Equal => {
                    // Key-equal ⇒ bit-equal (modulo -0.0), but NaN
                    // screens are never `==` — confirm before pairing.
                    if prev[prow as usize] == curr[crow as usize] {
                        self.prev_matched[prow as usize] = true;
                        self.matched_pos[crow as usize] = prow;
                        ci += 1;
                    }
                    pi += 1;
                }
            }
        }
    }

    /// Phase 3 — a still-unmatched curr row sharing a prev row's `hash`
    /// is the same shape somewhere else: emit both rects, the old place
    /// and the new. One with no partner is an add, so only its own rect
    /// goes out.
    ///
    /// **Requires `curr_keyed` sorted hash-major** — the property
    /// [`Self::begin`] establishes through [`PaintKey`]'s field order.
    /// Iterating it yields non-decreasing hashes, which is the whole
    /// reason a single never-reset forward cursor over `prev_keyed` can
    /// serve every curr row. Reset that cursor, or order either list any
    /// other way, and the pairing degrades silently.
    ///
    /// Sub-pixel float wobble on `Paint.screen` (the composer's pixel
    /// snapping runs downstream) makes strict `==` brittle, which is why
    /// a hash-only fallback exists at all — but it runs *after* the
    /// exact phase, so a shape that stayed put keeps its pairing even
    /// when another shape with the same hash moved within the node.
    ///
    /// Child markers can't push anything visible here: their screens are
    /// zero, so [`push_screen`] drops them. An added or removed child's
    /// pixels are damaged by its own node's tier instead.
    fn emit_moves_and_adds(&mut self, out: &mut Vec<Rect>, prev: &[Paint], curr: &[Paint]) {
        let mut pi = 0;
        for &(ck, crow) in &self.curr_keyed {
            if self.matched_pos[crow as usize] != ROW_UNMATCHED {
                continue;
            }
            while pi < self.prev_keyed.len() {
                let (pk, prow) = self.prev_keyed[pi];
                if self.prev_matched[prow as usize] || pk.hash < ck.hash {
                    pi += 1;
                } else {
                    break;
                }
            }
            match self.prev_keyed.get(pi) {
                Some(&(pk, prow)) if pk.hash == ck.hash => {
                    push_screen(out, prev[prow as usize].screen);
                    push_screen(out, curr[crow as usize].screen);
                    self.prev_matched[prow as usize] = true;
                    pi += 1;
                }
                _ => push_screen(out, curr[crow as usize].screen),
            }
        }
    }

    /// Phase 4 — prev rows no phase claimed are gone; clear their pixels.
    fn emit_removals(&self, out: &mut Vec<Rect>, prev: &[Paint]) {
        for (row, p) in prev.iter().enumerate() {
            if !self.prev_matched[row] {
                push_screen(out, p.screen);
            }
        }
    }
}

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
pub(super) fn has_order_inversion(matched_pos: &[u32]) -> bool {
    !matched_pos
        .iter()
        .filter(|&&pos| pos != ROW_UNMATCHED)
        .is_sorted()
}
