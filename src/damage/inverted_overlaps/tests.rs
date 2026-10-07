use crate::damage::inverted_overlaps::InvertedOverlaps;
use crate::damage::row_matcher::ROW_UNMATCHED;
use crate::primitives::geometry::rect::Rect;

fn damage(matched: &[u32], extents: &[Rect]) -> Vec<Rect> {
    let mut out = Vec::new();
    InvertedOverlaps::default().push(&mut out, matched, extents);
    out
}

/// Each row: last frame's positions, this frame's extents in order, and the rects pushed.
///
/// - Kept order or an added item: nothing.
/// - Two swapped: `b` damages its overlap with `a`, `[5, 10]²`.
/// - Reversed deck of three equal cards: each later card whole, once.
/// - `c` moves last to first: `a` damages its overlap with `c`, `[8, 10] × [0, 4]`; `b` (y = 5, below `c`) nothing.
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

/// The one rect may exceed the exact damage, never fall short: a wide item painting after a tall one it overlaps at its left end has exact damage `[0, 10] × [0, 10]`, but the union spans the whole wide item.
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
