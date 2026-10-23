//! Pairing one node's paint rows against last frame's, and turning the unpaired into damage.

use crate::cascade::paint::Paint;
use crate::common::block_arena::BlockArena;
use crate::common::non_max_u32::NonMaxU32;
use crate::common::span::Span;
use crate::damage;
use crate::primitives::geometry::rect::Rect;
use std::cmp::Ordering;

/// Result of [`RowMatcher::diff_changed_leg`].
#[derive(Debug)]
pub(super) struct ChangedLeg {
    /// Span covering this frame's paints: `prev_span` reused when the row count is stable, a fresh block otherwise.
    pub(super) span: Span,
    /// True when some pair of matched rows swapped relative order. Answered here because [`RowMatcher::matched_poss`] is retained scratch and on the fast path still holds an earlier node's answer.
    pub(super) order_inverted: bool,
}

/// Scratch for the content-keyed row matcher, and the phases that fill it. Phase order reads as a call sequence; every column keeps its capacity across frames.
#[derive(Debug, Default)]
pub(super) struct RowMatcher {
    /// Prev rows claimed by some phase.
    prev_matched: Vec<bool>,
    /// For each curr row, the prev row it paired with, `None` with no exact match in the prev span. Survives the call for the caller's order-inversion work.
    matched_pos: Vec<Option<NonMaxU32>>,
    /// `(key, row)` for each side's unclaimed rows, sorted: sort-and-merge keeps the all-rows-shifted case at O(n log n).
    prev_keyed: Vec<(PaintKey, u32)>,
    cur_keyed: Vec<(PaintKey, u32)>,
}

/// One node's paint rows across the frame boundary: the arena block holding last
/// frame's, and this frame's.
#[derive(Clone, Copy, Debug)]
pub(super) struct PaintsChange<'a> {
    pub(super) prev: Span,
    pub(super) cur: &'a [Paint],
}

impl RowMatcher {
    /// Prev row each curr row paired with on the last [`Self::diff_changed_leg`], `None` where none. Only read under [`ChangedLeg::order_inverted`], which no fast-path call sets.
    #[inline]
    pub(super) fn matched_poss(&self) -> &[Option<NonMaxU32>] {
        &self.matched_pos
    }

    /// Per-paint diff leg for the changed-paints arm, from the block `change.prev` names to `change.cur`.
    ///
    /// Fast path: a bit-identical positional match emits no damage and reuses the span.
    ///
    /// Slow path: exact `(screen, hash)` pairs first (no damage), then a same-`hash` unclaimed prev row means a move (both rects damaged), else an add. Unclaimed prev rows are removals. Hash-only matching exists because sub-pixel wobble on `Paint.screen` breaks strict `==`.
    ///
    /// Order check: swapped exact pairs still flip their overlap. This leg only reports it; the caller emits the overlap, which needs tree context.
    pub(super) fn diff_changed_leg(
        &mut self,
        paints: &mut BlockArena<Paint>,
        out: &mut Vec<Rect>,
        change: PaintsChange<'_>,
    ) -> ChangedLeg {
        let PaintsChange {
            prev: prev_span,
            cur: cur_paints,
        } = change;
        let prev_len = prev_span.len as usize;
        let prev = &paints.slots[prev_span.range()];

        if prev_len == cur_paints.len() && prev.iter().zip(cur_paints).all(|(p, c)| p == c) {
            return ChangedLeg {
                span: prev_span,
                order_inverted: false,
            };
        }

        self.reset_for(prev, cur_paints);
        self.claim_exact(prev, cur_paints);
        self.emit_moves_and_adds(out, prev, cur_paints);
        self.emit_removals(out, prev);

        let span = if prev_len == cur_paints.len() {
            paints.slots[prev_span.range()].copy_from_slice(cur_paints);
            prev_span
        } else {
            // Release before take so a count that moved within its size class reclaims its own block and a toggling shape never grows the arena. `cur_paints` is the cascade's buffer, so no aliasing.
            paints.release(prev_span);
            paints.store(cur_paints)
        };
        ChangedLeg {
            span,
            order_inverted: self.has_order_inversion(),
        }
    }

    /// Phase 1: reset, claim every same-index bit-identical pair, and key the rest.
    ///
    /// Leaves both keyed lists sorted by `(key, row)`: ascending rows claim ascending indices, and hash-major `PaintKey` lets [`Self::emit_moves_and_adds`] re-merge on hash alone.
    fn reset_for(&mut self, prev: &[Paint], cur: &[Paint]) {
        self.prev_matched.clear();
        self.prev_matched.resize(prev.len(), false);
        self.matched_pos.clear();
        self.matched_pos.resize(cur.len(), None);
        self.prev_keyed.clear();
        self.cur_keyed.clear();

        let shared = prev.len().min(cur.len());
        for row in 0..shared {
            let (p, c) = (prev[row], cur[row]);
            if p == c {
                self.prev_matched[row] = true;
                self.matched_pos[row] = Some(NonMaxU32::index(row));
            } else {
                self.prev_keyed.push((PaintKey::of(&p), row as u32));
                self.cur_keyed.push((PaintKey::of(&c), row as u32));
            }
        }
        for (offset, p) in prev[shared..].iter().enumerate() {
            self.prev_keyed
                .push((PaintKey::of(p), (shared + offset) as u32));
        }
        for (offset, c) in cur[shared..].iter().enumerate() {
            self.cur_keyed
                .push((PaintKey::of(c), (shared + offset) as u32));
        }
        self.prev_keyed.sort_unstable();
        self.cur_keyed.sort_unstable();
    }

    /// Phase 2: exact `(screen, hash)` pairs anywhere in the span. Emits no damage. Needs the sorted lists from [`Self::reset_for`].
    fn claim_exact(&mut self, prev: &[Paint], cur: &[Paint]) {
        let (mut pi, mut ci) = (0, 0);
        while pi < self.prev_keyed.len() && ci < self.cur_keyed.len() {
            let (pk, prow) = self.prev_keyed[pi];
            let (ck, crow) = self.cur_keyed[ci];
            match pk.cmp(&ck) {
                Ordering::Less => pi += 1,
                Ordering::Greater => ci += 1,
                Ordering::Equal => {
                    // Key-equal means bit-equal (modulo -0.0), but NaN screens are never `==`.
                    if prev[prow as usize] == cur[crow as usize] {
                        self.prev_matched[prow as usize] = true;
                        self.matched_pos[crow as usize] = Some(NonMaxU32::index(prow as usize));
                        ci += 1;
                    }
                    pi += 1;
                }
            }
        }
    }

    /// Phase 3: a curr row sharing a prev row's `hash` is the same shape moved, so both rects are damaged; one with no partner is an add and damages only its own rect.
    ///
    /// Requires `cur_keyed` sorted hash-major, so one never-reset forward cursor over `prev_keyed` serves every curr row. Child markers have zero screens and are dropped by [`damage::push_screen`].
    fn emit_moves_and_adds(&mut self, out: &mut Vec<Rect>, prev: &[Paint], cur: &[Paint]) {
        let mut pi = 0;
        for &(ck, crow) in &self.cur_keyed {
            if self.matched_pos[crow as usize].is_some() {
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
                    damage::push_screen(out, prev[prow as usize].screen);
                    damage::push_screen(out, cur[crow as usize].screen);
                    self.prev_matched[prow as usize] = true;
                    pi += 1;
                }
                _ => damage::push_screen(out, cur[crow as usize].screen),
            }
        }
    }

    /// Phase 4: prev rows no phase claimed are gone; damage them.
    fn emit_removals(&self, out: &mut Vec<Rect>, prev: &[Paint]) {
        for (row, p) in prev.iter().enumerate() {
            if !self.prev_matched[row] {
                damage::push_screen(out, p.screen);
            }
        }
    }

    /// True when matched prev positions are not non-decreasing in curr order. Equal neighbors cannot occur (each prev row is claimed once), so `is_sorted` is exact.
    fn has_order_inversion(&self) -> bool {
        !self
            .matched_pos
            .iter()
            .flatten()
            .map(|pos| pos.get())
            .is_sorted()
    }
}

/// Sort key: hash-major (one order serves both the exact and move passes), then the screen rect's bits with `-0.0` normalized. Key-equal rows are confirmed with `Paint ==` so NaN screens cannot false-pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PaintKey {
    hash: u64,
    screen_bits: [u32; 4],
}

impl PaintKey {
    fn of(p: &Paint) -> Self {
        // `f + 0.0` folds -0.0 onto +0.0 and leaves every other bit pattern, NaN included, unchanged.
        let n = |f: f32| (f + 0.0).to_bits();
        Self {
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

#[cfg(test)]
pub(crate) mod internals {
    use crate::damage::row_matcher::RowMatcher;
    use crate::internals::scratch_report;

    impl RowMatcher {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                prev_matched,
                matched_pos,
                prev_keyed,
                cur_keyed,
            } = self;
            scratch_report::vec_bytes(prev_matched)
                + scratch_report::vec_bytes(matched_pos)
                + scratch_report::vec_bytes(prev_keyed)
                + scratch_report::vec_bytes(cur_keyed)
        }
    }
}
