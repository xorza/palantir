//! Damage for items whose relative paint order flipped.

use crate::damage;
use crate::damage::row_matcher::ROW_UNMATCHED;
use crate::primitives::geometry::rect::Rect;

/// For each item, damages where it overlaps the items that painted before it now but after it last frame, whose stacking flipped.
///
/// One rect per item: its extent clamped to the union of those items' extents. This can exceed the exact union of pair overlaps, but the damage region merges into a few rects anyway and a superset only repaints; per-pair rects were `n² / 2` (half a million for 1000 reversed siblings).
///
/// The union of earlier items that painted later is a Fenwick-tree prefix over last frame's reversed positions, so the pass is `O(n log n)`.
#[derive(Debug, Default)]
pub(crate) struct InvertedOverlaps {
    /// One-based Fenwick tree: node `i` holds the union of extents added at reversed positions `(i - lowbit(i), i]`.
    unions: Vec<Rect>,
}

impl InvertedOverlaps {
    /// Damage into `out` the inversions among items whose last-frame positions are `matched` ([`ROW_UNMATCHED`] if absent) and whose extents are `extents`, both in this frame's paint order.
    pub(crate) fn push(&mut self, out: &mut Vec<Rect>, matched: &[u32], extents: &[Rect]) {
        debug_assert_eq!(matched.len(), extents.len());
        let Some(last) = matched
            .iter()
            .copied()
            .filter(|&p| p != ROW_UNMATCHED)
            .max()
        else {
            return;
        };
        self.unions.clear();
        self.unions.resize(last as usize + 2, Rect::ZERO);
        for (&position, &extent) in matched.iter().zip(extents) {
            if position == ROW_UNMATCHED {
                continue;
            }
            // Reversed, so items that came later last frame are a prefix.
            let reversed = (last - position) as usize;
            damage::push_screen(out, extent.clamp_to(self.prefix(reversed)));
            self.add(reversed, extent);
        }
    }

    /// The union of extents added at reversed positions below `end`.
    fn prefix(&self, end: usize) -> Rect {
        let mut union = Rect::ZERO;
        let mut i = end;
        while i > 0 {
            union = union.union(self.unions[i]);
            i &= i - 1;
        }
        union
    }

    fn add(&mut self, at: usize, extent: Rect) {
        let mut i = at + 1;
        while i < self.unions.len() {
            self.unions[i] = self.unions[i].union(extent);
            i += i.isolate_lowest_one();
        }
    }
}

#[cfg(test)]
mod tests;
