use super::{DAMAGE_RECT_CAP, DamageRegion};
use crate::primitives::rect::Rect;

fn collect(region: &DamageRegion) -> Vec<Rect> {
    region.iter().collect()
}

/// Sweep covering each branch of [`DamageRegion::add`]:
///
/// - empty input → no-op
/// - input contained by an existing rect → no-op
/// - input contains an existing rect → drops the contained slot
/// - LVGL rule fires (touching / overlapping) → merges into one
/// - distant disjoint inputs → kept as separate slots
/// - cascade absorption — adding a "bridge" rect coalesces neighbours
#[test]
fn add_policy_cases() {
    // empty
    let mut region = DamageRegion::default();
    region.add(Rect::new(10.0, 10.0, 0.0, 0.0));
    assert!(region.is_empty(), "empty rects must be ignored");

    // already covered (small inside big)
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 100.0, 100.0));
    region.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    assert_eq!(
        collect(&region),
        vec![Rect::new(0.0, 0.0, 100.0, 100.0)],
        "rect already inside an existing rect adds nothing",
    );

    // input contains existing — replace
    let mut region = DamageRegion::default();
    region.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    region.add(Rect::new(0.0, 0.0, 100.0, 100.0));
    assert_eq!(
        collect(&region),
        vec![Rect::new(0.0, 0.0, 100.0, 100.0)],
        "bigger rect should swallow the contained slot",
    );

    // axis-aligned overlap — bbox waste is small enough; LVGL rule fires
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(5.0, 0.0, 10.0, 10.0));
    assert_eq!(
        collect(&region),
        vec![Rect::new(0.0, 0.0, 15.0, 10.0)],
        "axis-aligned overlap should merge",
    );

    // edge-touching — non-strict ≤, still merges (one big run)
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(10.0, 0.0, 10.0, 10.0));
    assert_eq!(
        collect(&region),
        vec![Rect::new(0.0, 0.0, 20.0, 10.0)],
        "edge-touching pair should merge",
    );

    // Diagonal overlap with significant bbox waste stays separate.
    // bbox(A,B) = 225, |A|+|B| = 200 — rule rejects the merge so the
    // 25 px² of empty corners in the bbox aren't paid for.
    let mut region = DamageRegion::default();
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 10.0, 10.0);
    region.add(a);
    region.add(b);
    let rects = collect(&region);
    assert_eq!(
        rects.len(),
        2,
        "diagonal overlap with bbox waste must stay split: {rects:?}",
    );

    // distant disjoint — stay separate
    let mut region = DamageRegion::default();
    let a = Rect::new(0.0, 0.0, 5.0, 5.0);
    let b = Rect::new(995.0, 995.0, 5.0, 5.0);
    region.add(a);
    region.add(b);
    let rects = collect(&region);
    assert_eq!(rects.len(), 2, "far corners must not merge: {rects:?}");
    assert!(rects.contains(&a) && rects.contains(&b));

    // cascade — two distant rects stay split, then a "bridge" rect
    // overlapping both pulls them into one.
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region).len(), 2);
    region.add(Rect::new(0.0, 0.0, 110.0, 10.0));
    assert_eq!(
        collect(&region),
        vec![Rect::new(0.0, 0.0, 110.0, 10.0)],
        "bridge rect that contains both should collapse the region",
    );
}

/// Nine disjoint corner rects on a 1000×1000 surface: the first 8
/// fill the array, the 9th hits the cap and triggers the min-growth
/// merge into the geometrically nearest existing rect.
#[test]
fn nine_disjoint_corners_pick_min_growth_merge() {
    let mut region = DamageRegion::default();
    let corners = [
        Rect::new(0.0, 0.0, 5.0, 5.0),
        Rect::new(995.0, 0.0, 5.0, 5.0),
        Rect::new(0.0, 995.0, 5.0, 5.0),
        Rect::new(995.0, 995.0, 5.0, 5.0),
        Rect::new(495.0, 0.0, 5.0, 5.0),
        Rect::new(495.0, 995.0, 5.0, 5.0),
        Rect::new(0.0, 495.0, 5.0, 5.0),
        Rect::new(995.0, 495.0, 5.0, 5.0),
    ];
    for c in corners {
        region.add(c);
    }
    assert_eq!(region.iter().count(), DAMAGE_RECT_CAP);

    // Ninth rect lives next to the centre-top corner (495,0).
    // Min-growth picks that neighbour to merge with — distance to any
    // other slot is ≥ ~100 px on at least one axis.
    let extra = Rect::new(490.0, 5.0, 10.0, 5.0);
    region.add(extra);
    assert_eq!(region.iter().count(), DAMAGE_RECT_CAP);
    let rects = collect(&region);
    assert!(
        rects.iter().any(|r| {
            // Merged slot is the bbox of (495,0,5,5) ∪ (490,5,10,5).
            *r == Rect::new(490.0, 0.0, 10.0, 10.0)
        }),
        "expected min-growth merge with centre-top corner: {rects:?}",
    );
}

/// `total_area` sums per-rect areas without subtracting overlap. With
/// the merge policy in place, overlapping pairs collapse to one rect
/// before they reach the sum, so the over-count is bounded by the
/// "min-growth merged at cap" case.
#[test]
fn total_area_sums_disjoint_rects() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 100.0, 20.0, 20.0));
    assert_eq!(region.total_area(), 100.0 + 400.0);
}
