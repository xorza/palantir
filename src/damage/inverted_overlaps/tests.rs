use crate::damage::inverted_overlaps::InvertedOverlaps;
use crate::damage::row_matcher::ROW_UNMATCHED;
use crate::primitives::geometry::rect::Rect;

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
