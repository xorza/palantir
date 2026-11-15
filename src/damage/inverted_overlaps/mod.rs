//! Damage for items whose relative paint order flipped.

use crate::common::non_max::NonMaxU32;
use crate::damage;
use crate::primitives::geometry::rect::Rect;

/// For each item, damages where it overlaps the items that painted before it now but after it last frame, whose stacking flipped.
///
/// One rect per item: its extent clamped to the union of those items' extents. This can exceed the exact union of pair overlaps, but the damage region merges into a few rects anyway and a superset only repaints; per-pair rects were `n² / 2` (half a million for 1000 reversed siblings).
///
/// The union of earlier items that painted later is a Fenwick-tree prefix over last frame's reversed positions, so the pass is `O(n log n)`.
///
/// One scratch for every order diff of a frame, a node's rows and a plane's roots alike, as the
/// diffs run one after another: the extents a diff fills, then the tree it damages through.
#[derive(Debug, Default)]
pub(crate) struct InvertedOverlaps {
    /// The items' extents in this frame's paint order, which [`Self::cleared_extents`] hands out to
    /// fill.
    extents: Vec<Rect>,
    /// One-based Fenwick tree: node `i` holds the union of extents added at reversed positions `(i - lowbit(i), i]`.
    unions: Vec<Rect>,
}

impl InvertedOverlaps {
    /// The extents to fill for the next [`Self::push`], emptied of the last diff's.
    pub(crate) fn cleared_extents(&mut self) -> &mut Vec<Rect> {
        self.extents.clear();
        &mut self.extents
    }

    /// Damage into `out` the inversions among items whose last-frame positions are `matched` (`None` if absent) and whose extents [`Self::cleared_extents`] was filled with, both in this frame's paint order.
    pub(crate) fn push(&mut self, out: &mut Vec<Rect>, matched: &[Option<NonMaxU32>]) {
        debug_assert_eq!(matched.len(), self.extents.len());
        let Some(last) = matched.iter().flatten().map(|pos| pos.get()).max() else {
            return;
        };
        let Self { extents, unions } = self;
        unions.clear();
        unions.resize(last as usize + 2, Rect::ZERO);
        for (&pos, &extent) in matched.iter().zip(extents.iter()) {
            let Some(pos) = pos else {
                continue;
            };
            // Reversed, so items that came later last frame are a prefix.
            let reversed = (last - pos.get()) as usize;
            damage::push_screen(out, extent.clamp_to(prefix(unions, reversed)));
            add(unions, reversed, extent);
        }
    }
}

/// The union of extents added at reversed positions below `end`, from the Fenwick tree `unions`.
const fn prefix(unions: &[Rect], end: usize) -> Rect {
    let mut union = Rect::ZERO;
    let mut i = end;
    while i > 0 {
        union = union.union(unions[i]);
        i &= i - 1;
    }
    union
}

/// Add `extent` at reversed position `at` to the Fenwick tree `unions`.
const fn add(unions: &mut [Rect], at: usize, extent: Rect) {
    let mut i = at + 1;
    while i < unions.len() {
        unions[i] = unions[i].union(extent);
        i += i.isolate_lowest_one();
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::damage::inverted_overlaps::InvertedOverlaps;
    use crate::internals::scratch_report;

    impl InvertedOverlaps {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self { extents, unions } = self;
            scratch_report::vec_bytes(extents) + scratch_report::vec_bytes(unions)
        }
    }
}

#[cfg(test)]
mod tests;
