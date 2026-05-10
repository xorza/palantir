use super::{DAMAGE_RECT_CAP, DEFAULT_PASS_BUDGET_PX, DamageRegion};
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

/// A rect already covered by an existing slot adds nothing (the
/// `contains` early-return short-circuits before the cluster-grow
/// loop runs).
#[test]
fn add_already_covered_is_noop() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 100.0, 100.0));
    region.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 100.0, 100.0)]);
}

/// A rect that contains an existing slot replaces it — caught by
/// the cluster-grow loop (cost = `−existing.area()` < 0 < budget).
#[test]
fn add_swallows_contained_existing() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    region.add(Rect::new(0.0, 0.0, 100.0, 100.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 100.0, 100.0)]);
}

/// Axis-aligned overlap: bbox 15×10 = 150, sum 200, cost = −50 →
/// merge.
#[test]
fn add_merges_axis_aligned_overlap() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(5.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 15.0, 10.0)]);
}

/// Edge-touching pair: bbox 200, sum 200, cost 0 → merge.
#[test]
fn add_merges_edge_touching() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(10.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 20.0, 10.0)]);
}

/// Near-disjoint pair (gap 2): bbox 220, sum 200, cost 20 — well
/// under default budget → merge.
#[test]
fn add_merges_near_disjoint() {
    let mut region = DamageRegion::default();
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(12.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 22.0, 10.0)]);
}

/// Diagonal-overlap pair: bbox 225, union 175 (overlap 25), cost
/// −25 → merge.
#[test]
fn add_merges_diagonal_overlap() {
    let mut region = DamageRegion::default();
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 10.0, 10.0);
    region.add(a);
    region.add(b);
    assert_eq!(collect(&region), vec![a.union(b)]);
}

/// Pair whose merge cost exceeds a tight budget stays split.
/// 10×10 rects, gap 15 → bbox 350, sum 200, cost 150 > 100 budget.
#[test]
fn add_keeps_pair_above_budget_split() {
    let mut region = DamageRegion::with_budget(100.0);
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(25.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region).len(), 2);
}

/// Distant disjoint rects (the corner-pair pathology) stay split
/// at any reasonable budget. Cost ≈ 1 000 000, way above any
/// per-pass budget we'd ship.
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

/// Cluster-grow: a "bridge" rect that contains two previously-
/// disjoint slots collapses the region. Tight budget keeps the
/// initial pair split (cost 900 > 50); adding the bridge then
/// swallows both (each contained → cost = −existing.area()).
#[test]
fn add_cascade_absorbs_through_bridge() {
    let mut region = DamageRegion::with_budget(50.0);
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 0.0, 10.0, 10.0));
    assert_eq!(collect(&region).len(), 2);
    region.add(Rect::new(0.0, 0.0, 110.0, 10.0));
    assert_eq!(collect(&region), vec![Rect::new(0.0, 0.0, 110.0, 10.0)]);
}

/// At the cap, the ninth rect triggers the min-growth fallback.
/// Strict-overlap-only budget keeps the eight corners split so the
/// ninth hits the fallback. Exact merge target is unstable
/// (tie-breaking depends on iteration order); we only pin (a) the
/// cap holds, (b) some slot now equals the bbox of the colliding
/// pair.
#[test]
fn nine_disjoint_corners_min_growth_at_cap() {
    let mut region = DamageRegion::with_budget(0.0);
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

/// Compact cluster of four small rects: pairwise / cluster-grow
/// costs all sit well below the default budget, so the
/// agglomerative loop collapses them gradually to one bbox.
/// Sanity-check that the cluster path actually fires.
#[test]
fn compact_cluster_of_four_collapses_at_default_budget() {
    let mut region = DamageRegion::default();
    assert_eq!(region.budget_px, DEFAULT_PASS_BUDGET_PX);
    for r in [
        Rect::new(100.0, 100.0, 50.0, 50.0),
        Rect::new(200.0, 100.0, 50.0, 50.0),
        Rect::new(100.0, 200.0, 50.0, 50.0),
        Rect::new(200.0, 200.0, 50.0, 50.0),
    ] {
        region.add(r);
    }
    assert_eq!(
        collect(&region),
        vec![Rect::new(100.0, 100.0, 150.0, 150.0)],
    );
}

/// Screenshot regression: four rects approximating the "popup tab"
/// damage overlay (`docs/screens/Screenshot 2026-05-10 at
/// 21.27.14.png`). At the default budget every pairwise cost
/// (≥ ~45 K px²) sits above DEFAULT_PASS_BUDGET_PX, so the
/// algorithm has nothing to merge — pinned to document the
/// limitation. The matching `*_under_high_budget` test pins the
/// other knob position.
#[test]
fn screenshot_cluster_at_default_budget_stays_split() {
    let rs = [
        Rect::new(80.0, 300.0, 80.0, 230.0),
        Rect::new(260.0, 360.0, 140.0, 70.0),
        Rect::new(260.0, 510.0, 230.0, 20.0),
        Rect::new(80.0, 580.0, 170.0, 20.0),
    ];
    let mut region = DamageRegion::default();
    for r in rs {
        region.add(r);
    }
    assert_eq!(collect(&region).len(), rs.len());
}

/// Same fixture, budget cranked up high. Confirms the per-region
/// budget knob actually drives full collapse for callers that want
/// aggressive merging (e.g. a TBDR mobile target where every extra
/// pass is expensive).
#[test]
fn cluster_of_four_collapses_under_high_budget() {
    let rs = [
        Rect::new(80.0, 300.0, 80.0, 230.0),
        Rect::new(260.0, 360.0, 140.0, 70.0),
        Rect::new(260.0, 510.0, 230.0, 20.0),
        Rect::new(80.0, 580.0, 170.0, 20.0),
    ];
    let mut region = DamageRegion::with_budget(60_000.0);
    for r in rs {
        region.add(r);
    }
    let bbox = rs.iter().copied().reduce(|a, b| a.union(b)).unwrap();
    assert_eq!(collect(&region), vec![bbox]);
}

/// Same fixture, budget cranked down to the 2-cell GPU-bench
/// crossover (~7 000 px²): every pairwise cost is above this, so
/// the rects stay split. Pins the lower end of the tweakable knob.
#[test]
fn screenshot_cluster_stays_split_at_tight_budget() {
    let rs = [
        Rect::new(80.0, 300.0, 80.0, 230.0),
        Rect::new(260.0, 360.0, 140.0, 70.0),
        Rect::new(260.0, 510.0, 230.0, 20.0),
        Rect::new(80.0, 580.0, 170.0, 20.0),
    ];
    let mut region = DamageRegion::with_budget(7_000.0);
    for r in rs {
        region.add(r);
    }
    assert_eq!(collect(&region).len(), 4);
}

/// `total_area` sums per-rect areas without subtracting overlap.
/// With the merge policy, overlapping pairs collapse before they
/// reach the sum; this disjoint case is the contract the
/// full-repaint heuristic relies on. Strict-overlap budget keeps
/// the pair from merging.
#[test]
fn total_area_sums_disjoint_rects() {
    let mut region = DamageRegion::with_budget(0.0);
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 100.0, 20.0, 20.0));
    assert_eq!(region.total_area(), 100.0 + 400.0);
}
