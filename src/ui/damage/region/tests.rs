use super::{DAMAGE_RECT_CAP, DamageRegion};
use crate::primitives::rect::Rect;

fn collect(region: &DamageRegion) -> Vec<Rect> {
    region.iter_rects().collect()
}

/// `add` ignores zero-area input — empty rects contribute nothing.
#[test]
fn add_empty_is_noop() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(10.0, 10.0, 0.0, 0.0));
    assert!(region.is_empty());
}

/// Step 2: a rect already covered by an existing slot adds nothing.
#[test]
fn add_already_covered_is_noop() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 100.0, 100.0));
    region.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 100.0, 100.0)]);
}

/// Step 3: a rect that contains an existing slot replaces it.
#[test]
fn add_swallows_contained_existing() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    region.add(Rect::new(0.0, 0.0, 100.0, 100.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 100.0, 100.0)]);
}

/// Proximity rule fires for axis-aligned overlap: bbox area
/// (150) is well under `MERGE_AREA_RATIO × (100 + 100) = 260`.
#[test]
fn add_merges_axis_aligned_overlap() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(5.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 15.0, 10.0)]);
}

/// Edge-touching pairs merge (bbox = 200 = sum, well under
/// the 1.3× threshold).
#[test]
fn add_merges_edge_touching() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(10.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 20.0, 10.0)]);
}

/// Near-but-not-overlapping pairs merge under the 1.3× ratio.
/// Two 10×10 rects 2 px apart: bbox = 22×10 = 220, sum = 200,
/// ratio 1.10 ≤ 1.30. The strict-overlap rule rejected this; the
/// new rule accepts.
#[test]
fn add_merges_near_disjoint_under_ratio() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(12.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 22.0, 10.0)]);
}

/// Diagonal overlap (15×15 bbox = 225, sum = 200, ratio 1.125)
/// merges under the 1.3× rule. The strict-overlap rule rejected
/// this case.
#[test]
fn add_merges_diagonal_overlap_under_ratio() {
    let mut region = DamageRegion::default();
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 10.0, 10.0);
    region.add(a);
    region.add(b);
    assert_eq!(collect(&region), vec![a.union(b)]);
}

/// Pair just over the 1.3× ceiling stays split: two 10×10 rects
/// 7 px apart give bbox = 27×10 = 270, sum = 200, ratio 1.35 > 1.30.
#[test]
fn add_keeps_pair_above_ratio_split() {
    let mut region = DamageRegion::default();
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(17.0, 0.0, 10.0, 10.0);
    region.add(a);
    region.add(b);
    assert_eq!(collect(&region).len(), 2);
}

/// Distant disjoint rects (the corner-pair pathology) stay split.
#[test]
fn add_keeps_far_corners_split() {
    let mut region = DamageRegion::default();
    let a = Rect::new(0.0, 0.0, 5.0, 5.0);
    let b = Rect::new(995.0, 995.0, 5.0, 5.0);
    region.add(a);
    region.add(b);
    let rects = collect(&region);
    assert_eq!(rects.len(), 2);
    assert!(rects.contains(&a) && rects.contains(&b));
}

/// Cascade: a "bridge" rect that contains two previously-disjoint
/// rects collapses the region. This is the path that justifies the
/// `loop` in `add` — absorbing one rect lets the candidate absorb the
/// next.
#[test]
fn add_cascade_absorbs_through_bridge() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region).len(), 2);
    region.add(Rect::new(0.0, 0.0, 110.0, 10.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 110.0, 10.0)]);
}

/// Step 4: at the cap, the ninth rect triggers the min-growth fallback.
/// The exact merge target is unstable (multiple slots can produce
/// identical growth depending on iteration order); we only pin that
/// (a) the cap holds, and (b) some slot now equals the bbox of the
/// colliding pair.
#[test]
fn nine_disjoint_corners_min_growth_at_cap() {
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
    assert_eq!(region.iter_rects().count(), DAMAGE_RECT_CAP);

    // Ninth rect overlaps the centre-top corner; that's the unique
    // min-growth target (other slots are ≥ ~100 px away on at least
    // one axis).
    let extra = Rect::new(490.0, 5.0, 10.0, 5.0);
    region.add(extra);
    assert_eq!(region.iter_rects().count(), DAMAGE_RECT_CAP);
    let merged = Rect::new(495.0, 0.0, 5.0, 5.0).union(extra);
    let rects = collect(&region);
    assert!(
        rects.contains(&merged),
        "expected the bbox of the colliding pair as one slot: {rects:?}",
    );
}

/// `total_area` sums per-rect areas without subtracting overlap.
/// With the merge policy, overlapping pairs collapse before they
/// reach the sum; this disjoint case is the contract the
/// full-repaint heuristic relies on.
#[test]
fn total_area_sums_disjoint_rects() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 100.0, 20.0, 20.0));
    assert_eq!(region.total_area(), 100.0 + 400.0);
}
