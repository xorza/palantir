use crate::common::non_max_u32::NonMaxU32;
use crate::damage::inverted_overlaps::InvertedOverlaps;
use crate::primitives::geometry::rect::Rect;

/// `matched` as last frame's positions, `None` for an added item.
fn damage(matched: &[Option<u32>], extents: &[Rect]) -> Vec<Rect> {
    let matched: Vec<_> = matched
        .iter()
        .map(|pos| pos.map(|pos| NonMaxU32::new(pos).unwrap()))
        .collect();
    let mut out = Vec::new();
    InvertedOverlaps::default().push(&mut out, &matched, extents);
    out
}

/// One case of [`one_rect_per_item_over_the_flipped_items`].
#[derive(Debug)]
struct Flip<'a> {
    /// Last frame's positions, `None` for an added item.
    matched: &'a [Option<u32>],
    /// This frame's extents, in paint order.
    extents: &'a [Rect],
    /// The rects pushed.
    expected: &'a [Rect],
}

/// Each case:
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
    let cases: [Flip<'_>; 5] = [
        Flip {
            matched: &[Some(0), Some(1)],
            extents: &[a, b],
            expected: &[],
        },
        Flip {
            matched: &[None, Some(0), Some(1)],
            extents: &[c, a, b],
            expected: &[],
        },
        Flip {
            matched: &[Some(1), Some(0)],
            extents: &[a, b],
            expected: &[Rect::new(5.0, 5.0, 5.0, 5.0)],
        },
        Flip {
            matched: &[Some(2), Some(1), Some(0)],
            extents: &[card, card, card],
            expected: &[card, card],
        },
        Flip {
            matched: &[Some(2), Some(0), Some(1)],
            extents: &[c, a, b],
            expected: &[Rect::new(8.0, 0.0, 2.0, 4.0)],
        },
    ];
    for Flip {
        matched,
        extents,
        expected,
    } in cases
    {
        assert_eq!(damage(matched, extents), expected, "{matched:?}");
    }
}

/// The one rect may exceed the exact damage, never fall short: a wide item painting after a tall one it overlaps at its left end has exact damage `[0, 10] × [0, 10]`, but the union spans the whole wide item.
#[test]
fn the_union_damages_a_superset() {
    let tall = Rect::new(0.0, 0.0, 10.0, 100.0);
    let far = Rect::new(90.0, 50.0, 10.0, 50.0);
    let wide = Rect::new(0.0, 0.0, 100.0, 10.0);
    let out = damage(&[Some(1), Some(2), Some(0)], &[tall, far, wide]);
    assert_eq!(out, [wide]);
    assert!(out[0].contains_rect(tall.clamp_to(wide)));
    assert!(far.clamp_to(wide).is_paint_empty(), "premise: far misses");
}
