//! Damage for items whose relative paint order flipped.

use crate::primitives::rect::Rect;
use crate::scene::damage;
use crate::scene::damage::row_matcher::ROW_UNMATCHED;

/// Damages, for each item, where it overlaps the items that painted
/// before it now but after it last frame — the pixels whose stacking
/// that flip changed.
///
/// One rect per item: its extent clamped to the union of those items'
/// extents. The exact damage is the union of each pair's overlap, so the
/// clamp of the union can only be larger — it adds the parts of the item
/// that the union's bounding box reaches without any one of them
/// overlapping there. The damage region merges into a few bounding
/// rects anyway, and a superset only repaints; one rect per item is what
/// bounds the frame. A pair at a time pushed `n² / 2` rects for `n`
/// reversed siblings, half a million for a reversed deck of 1000.
///
/// The union of the earlier items that painted later is a Fenwick-tree
/// prefix over last frame's positions, reversed, so the whole pass is
/// `O(n log n)`.
#[derive(Debug, Default)]
pub(crate) struct InvertedOverlaps {
    /// One-based Fenwick tree: node `i` holds the union of the extents
    /// added at the reversed positions `(i - lowbit(i), i]`.
    unions: Vec<Rect>,
}

impl InvertedOverlaps {
    /// Damage into `out` the inversions among items whose last-frame
    /// positions are `matched` — [`ROW_UNMATCHED`] for an item last frame
    /// lacked — and whose extents this frame are `extents`, both in this
    /// frame's paint order.
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
            // Reversed, so the items that came later last frame are a
            // prefix.
            let reversed = (last - position) as usize;
            damage::push_screen(out, extent.clamp_to(self.prefix(reversed)));
            self.add(reversed, extent);
        }
    }

    /// The union of the extents added at reversed positions below `end`.
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
mod tests {
    use crate::primitives::rect::Rect;
    use crate::scene::damage::inverted_overlaps::InvertedOverlaps;
    use crate::scene::damage::row_matcher::ROW_UNMATCHED;

    fn damage(matched: &[u32], extents: &[Rect]) -> Vec<Rect> {
        let mut out = Vec::new();
        InvertedOverlaps::default().push(&mut out, matched, extents);
        out
    }

    /// Each row: last frame's positions, this frame's extents in this
    /// frame's order, and the rects pushed.
    ///
    /// - Kept order damages nothing, and neither does an added item.
    /// - Two items swapped: the second now (`b`, first before) damages its
    ///   overlap with `a`, `[5, 10]²`.
    /// - A reversed deck of three equal cards damages each later card
    ///   whole, once: the first has nothing before it.
    /// - `c` came last before and comes first now, ahead of `a` and `b`,
    ///   which kept their order. `a` damages its overlap with `c`,
    ///   `[8, 10] × [0, 4]`; `b` starts at y = 5, below `c`, and damages
    ///   nothing.
    #[test]
    fn one_rect_per_item_over_the_flipped_items() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        let c = Rect::new(8.0, 0.0, 10.0, 4.0);
        let card = Rect::new(0.0, 0.0, 20.0, 20.0);
        let cases: [(&[u32], &[Rect], &[Rect]); 5] = [
            (&[0, 1], &[a, b], &[]),
            (&[ROW_UNMATCHED, 0, 1], &[c, a, b], &[]),
            (&[1, 0], &[a, b], &[Rect::new(5.0, 5.0, 5.0, 5.0)]),
            (&[2, 1, 0], &[card, card, card], &[card, card]),
            (&[2, 0, 1], &[c, a, b], &[Rect::new(8.0, 0.0, 2.0, 4.0)]),
        ];
        for (matched, extents, expected) in cases {
            assert_eq!(damage(matched, extents), expected, "{matched:?}");
        }
    }

    /// The one rect can exceed the exact damage, never fall short of it.
    /// A wide item now paints after a tall one it overlaps at its left
    /// end and a far one it does not overlap at all. The exact damage is
    /// the left end, `[0, 10] × [0, 10]`; the union of the two reaches
    /// across the whole wide item, so all of it is damaged.
    #[test]
    fn the_union_damages_a_superset() {
        let tall = Rect::new(0.0, 0.0, 10.0, 100.0);
        let far = Rect::new(90.0, 50.0, 10.0, 50.0);
        let wide = Rect::new(0.0, 0.0, 100.0, 10.0);
        let out = damage(&[1, 2, 0], &[tall, far, wide]);
        assert_eq!(out, [wide]);
        assert!(out[0].contains_rect(tall.clamp_to(wide)));
        assert!(far.clamp_to(wide).is_paint_empty(), "premise: far misses");
    }
}
