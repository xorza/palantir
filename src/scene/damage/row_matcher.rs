//! Pairing one node's paint rows against last frame's, and turning what
//! did not pair into damage.

use crate::common::block_arena::BlockArena;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::scene::cascade::paint::Paint;
use crate::scene::damage::push_screen;
use std::cmp::Ordering;

/// `matched_pos` sentinel for a curr row with no exact match in the
/// prev span (moved / added / content-changed — the content diff
/// damages those over their full rects).
pub(super) const ROW_UNMATCHED: u32 = u32::MAX;

/// Result of [`RowMatcher::diff_changed_leg`].
#[derive(Debug)]
pub(super) struct ChangedLeg {
    /// Span covering this frame's paints — `prev_span` reused when the
    /// row count is stable, a freshly taken block when it changes.
    pub(super) span: Span,
    /// True when some pair of matched rows swapped relative order.
    ///
    /// Answered here rather than left for the caller to ask, because the
    /// question is only meaningful about the pairing *this* call
    /// produced: [`RowMatcher::matched_positions`] is retained scratch,
    /// and on the fast path it still holds some earlier node's answer.
    /// A caller that reads it only when this is `true` cannot get the
    /// stale one.
    pub(super) order_inverted: bool,
}

/// Scratch for the content-keyed row matcher, and the phases that fill
/// it.
///
/// One type rather than four loose `Vec`s so the phase order reads as a
/// call sequence and each phase's doc can state what the previous one
/// left behind. The sort order in particular is load-bearing twice over,
/// and stating it on [`PaintKey`] alone would leave it forty lines from
/// the merge that depends on it.
///
/// Every column keeps its capacity across frames, so a steady-state
/// content reshuffle allocates nothing.
#[derive(Debug, Default)]
pub(super) struct RowMatcher {
    /// Which prev rows have been claimed by some phase.
    prev_matched: Vec<bool>,
    /// For each curr row, the prev row it paired with, or
    /// [`ROW_UNMATCHED`]. Survives the call for the caller's
    /// order-inversion work: an exact pair emits no content damage, but
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
    /// Which prev row each curr row paired with on the last
    /// [`Self::diff_changed_leg`], for the caller's overlap enumeration.
    /// `ROW_UNMATCHED` where nothing paired.
    ///
    /// Only read under [`ChangedLeg::order_inverted`], which no fast-path
    /// call can set — that is what keeps a caller from reading the
    /// previous node's pairing.
    #[inline]
    pub(super) fn matched_positions(&self) -> &[u32] {
        &self.matched_pos
    }

    /// Per-paint diff leg for the changed-paints arm, against the block
    /// `prev_span` names in `paints`. Three strategies in order of cost:
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
    /// these row reorders). This leg only *reports* that it happened;
    /// the caller enumerates the pairs and emits each one's extent
    /// overlap, because child-marker extents need tree context no part
    /// of this file holds.
    ///
    /// Pass 1's positional pre-pass pairs in-place rows in O(n); only
    /// the leftovers pay the keyed sort + merge. The retained scratch
    /// keeps every pass alloc-free across frames; empty leftovers (every
    /// shape paired positionally) make both merges trivially skip. A
    /// stable row count refreshes the existing block in place; a changed
    /// one hands the old block back to its size class and takes one for
    /// the new length.
    pub(super) fn diff_changed_leg(
        &mut self,
        paints: &mut BlockArena<Paint>,
        out: &mut Vec<Rect>,
        prev_span: Span,
        curr_paints: &[Paint],
    ) -> ChangedLeg {
        let prev_start = prev_span.start as usize;
        let prev_len = prev_span.len as usize;
        let prev_slice = &paints.slots[prev_start..prev_start + prev_len];

        if prev_len == curr_paints.len() && prev_slice.iter().zip(curr_paints).all(|(p, c)| p == c)
        {
            return ChangedLeg {
                span: prev_span,
                order_inverted: false,
            };
        }

        let prev = &paints.slots[prev_start..prev_start + prev_len];
        self.begin(prev, curr_paints);
        self.claim_exact(prev, curr_paints);
        self.emit_moves_and_adds(out, prev, curr_paints);
        self.emit_removals(out, prev);

        let span = if prev_len == curr_paints.len() {
            paints.slots[prev_span.range()].copy_from_slice(curr_paints);
            prev_span
        } else {
            // A row-count change is a different size class as often as
            // not, so this is a release-then-take rather than a resize.
            // Release first: a count that moved within its class — a
            // 40-shape node dropping to 39 — then reclaims *its own*
            // block, which is what keeps a shape toggled every frame
            // from growing the arena at all after warm-up. The old block
            // is unreachable from the moment this returns the new span,
            // and `curr_paints` is the cascade's buffer, not ours, so
            // handing it back before the copy cannot alias anything.
            paints.release(prev_span);
            paints.store(curr_paints)
        };
        ChangedLeg {
            span,
            order_inverted: self.has_order_inversion(),
        }
    }

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

    /// True when some pair of matched rows inverted its relative order —
    /// i.e. the matched prev positions aren't non-decreasing in curr
    /// order. O(n) gate in front of the caller's quadratic pair
    /// enumeration. Equal adjacent positions can't occur (each prev row
    /// is claimed at most once), so allow-equal `is_sorted` is exact.
    fn has_order_inversion(&self) -> bool {
        !self
            .matched_pos
            .iter()
            .filter(|&&pos| pos != ROW_UNMATCHED)
            .is_sorted()
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
