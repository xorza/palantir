use crate::damage::region::{DAMAGE_RECT_CAP, DEFAULT_PASS_BUDGET_PX, DamageRegion};
use crate::primitives::geometry::rect::Rect;

/// A region and the budget its case folds under (an argument of [`DamageRegion::add`]).
#[derive(Debug)]
struct Fold {
    region: DamageRegion,
    budget_px: f32,
}

impl Fold {
    fn new(budget_px: f32) -> Self {
        Self {
            region: DamageRegion::default(),
            budget_px,
        }
    }

    fn default_budget() -> Self {
        Self::new(DEFAULT_PASS_BUDGET_PX)
    }

    fn add(&mut self, r: Rect) {
        self.region.add(r, self.budget_px);
    }

    fn rects(&self) -> Vec<Rect> {
        self.region.iter_rects().collect()
    }

    fn len(&self) -> usize {
        self.region.iter_rects().count()
    }
}

#[test]
fn add_empty_is_noop() {
    let mut region = Fold::default_budget();
    region.add(Rect::new(10.0, 10.0, 0.0, 0.0));
    assert!(region.rects().is_empty());
}

/// A rect already covered by an existing slot adds nothing.
#[test]
fn add_already_covered_is_noop() {
    let mut region = Fold::default_budget();
    region.add(Rect::new(0.0, 0.0, 100.0, 100.0));
    region.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    assert_eq!(region.rects(), vec![Rect::new(0.0, 0.0, 100.0, 100.0)]);
}

/// A rect containing an existing slot replaces it.
#[test]
fn add_swallows_contained_existing() {
    let mut region = Fold::default_budget();
    region.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    region.add(Rect::new(0.0, 0.0, 100.0, 100.0));
    assert_eq!(region.rects(), vec![Rect::new(0.0, 0.0, 100.0, 100.0)]);
}

/// Pairs the SAH cost accepts under the default budget merge into one rect.
#[test]
fn add_merges_pair_under_budget() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let cases: &[(&str, Rect, Rect, Rect)] = &[
        (
            "axis_aligned_overlap",
            a,
            Rect::new(5.0, 0.0, 10.0, 10.0),
            Rect::new(0.0, 0.0, 15.0, 10.0),
        ),
        (
            "edge_touching",
            a,
            Rect::new(10.0, 0.0, 10.0, 10.0),
            Rect::new(0.0, 0.0, 20.0, 10.0),
        ),
        (
            "near_disjoint_gap2",
            a,
            Rect::new(12.0, 0.0, 10.0, 10.0),
            Rect::new(0.0, 0.0, 22.0, 10.0),
        ),
        (
            "diagonal_overlap",
            a,
            Rect::new(5.0, 5.0, 10.0, 10.0),
            a.union(Rect::new(5.0, 5.0, 10.0, 10.0)),
        ),
    ];
    for (label, p, q, want) in cases {
        let mut region = Fold::default_budget();
        region.add(*p);
        region.add(*q);
        assert_eq!(region.rects(), vec![*want], "case: {label}");
    }
}

/// A pair whose merge cost exceeds a tight budget stays split.
#[test]
fn add_keeps_pair_above_budget_split() {
    let mut region = Fold::new(100.0);
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(25.0, 0.0, 10.0, 10.0));
    assert_eq!(region.rects().len(), 2);
}

/// An intersecting pair always merges, even at the tightest budget: separate
/// overlapping scissor passes would paint the overlap twice.
#[test]
fn intersecting_pair_merges_at_zero_budget() {
    let mut region = Fold::new(0.0);
    // The bbox far exceeds the areas' sum, so only the intersect override merges them.
    let a = Rect::new(40.0, 40.0, 250.0, 600.0);
    let b = Rect::new(40.0, 140.0, 1450.0, 100.0);
    region.add(a);
    region.add(b);
    assert_eq!(region.rects(), vec![a.union(b)]);
}

/// Distant disjoint rects stay split at any reasonable budget.
#[test]
fn add_keeps_far_corners_split() {
    let mut region = Fold::default_budget();
    let a = Rect::new(0.0, 0.0, 5.0, 5.0);
    let b = Rect::new(995.0, 995.0, 5.0, 5.0);
    region.add(a);
    region.add(b);
    let rects = region.rects();
    assert_eq!(rects.len(), 2);
    assert!(rects.contains(&a) && rects.contains(&b));
}

/// Cluster-grow: a "bridge" rect containing two disjoint slots collapses the region.
#[test]
fn add_cascade_absorbs_through_bridge() {
    let mut region = Fold::new(50.0);
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 0.0, 10.0, 10.0));
    assert_eq!(region.rects().len(), 2);
    region.add(Rect::new(0.0, 0.0, 110.0, 10.0));
    assert_eq!(region.rects(), vec![Rect::new(0.0, 0.0, 110.0, 10.0)]);
}

/// At the cap, the ninth rect triggers the min-growth fallback, which must respect the cap and re-absorb overlapped slots.
#[test]
fn min_growth_at_cap_reabsorbs_new_overlaps() {
    let mut region = Fold::new(0.0);
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
    assert_eq!(region.len(), DAMAGE_RECT_CAP);

    let extra = Rect::new(490.0, 5.0, 10.0, 5.0);
    region.add(extra);
    assert_eq!(region.len(), DAMAGE_RECT_CAP);
    let merged = Rect::new(495.0, 0.0, 5.0, 5.0).union(extra);
    let rects = region.rects();
    assert!(
        rects.contains(&merged),
        "expected the bbox of the colliding pair as one slot: {rects:?}",
    );

    let target = Rect::new(0.0, 0.0, 10.0, 1000.0);
    let newly_overlapped = Rect::new(11.0, 900.0, 1.0, 1100.0);
    let fillers = [
        Rect::new(10_000.0, 10_000.0, 1.0, 1.0),
        Rect::new(20_000.0, 10_000.0, 1.0, 1.0),
        Rect::new(30_000.0, 10_000.0, 1.0, 1.0),
        Rect::new(40_000.0, 10_000.0, 1.0, 1.0),
        Rect::new(50_000.0, 10_000.0, 1.0, 1.0),
        Rect::new(60_000.0, 10_000.0, 1.0, 1.0),
    ];
    let mut region = Fold::new(0.0);
    region.add(target);
    region.add(newly_overlapped);
    for filler in fillers {
        region.add(filler);
    }
    assert_eq!(region.len(), DAMAGE_RECT_CAP);

    let extra = Rect::new(20.0, 0.0, 10.0, 10.0);
    region.add(extra);
    let absorbed = target.union(extra).union(newly_overlapped);
    let rects = region.rects();
    assert_eq!(rects.len(), DAMAGE_RECT_CAP - 1);
    assert!(
        rects.contains(&absorbed),
        "missing absorbed bbox: {rects:?}"
    );
    for filler in fillers {
        assert!(
            rects.contains(&filler),
            "missing filler {filler:?}: {rects:?}"
        );
    }
    for (i, rect) in rects.iter().enumerate() {
        for other in &rects[i + 1..] {
            assert!(
                !rect.intersects(*other),
                "retained rects overlap: {rect:?}, {other:?}"
            );
        }
    }
}

/// Four compact small rects collapse to one bbox under the default budget.
#[test]
fn compact_cluster_of_four_collapses_at_default_budget() {
    let mut region = Fold::default_budget();
    for r in [
        Rect::new(100.0, 100.0, 50.0, 50.0),
        Rect::new(200.0, 100.0, 50.0, 50.0),
        Rect::new(100.0, 200.0, 50.0, 50.0),
        Rect::new(200.0, 200.0, 50.0, 50.0),
    ] {
        region.add(r);
    }
    assert_eq!(region.rects(), vec![Rect::new(100.0, 100.0, 150.0, 150.0)],);
}

/// Four rects approximating the "popup tab" overlay, swept across the budget:
/// all stay split at the default and 7 000 px²; 60 000 collapses them.
#[test]
fn screenshot_cluster_budget_sweep() {
    let rs = [
        Rect::new(80.0, 300.0, 80.0, 230.0),
        Rect::new(260.0, 360.0, 140.0, 70.0),
        Rect::new(260.0, 510.0, 230.0, 20.0),
        Rect::new(80.0, 580.0, 170.0, 20.0),
    ];
    let bbox = rs.iter().copied().reduce(Rect::union).unwrap();
    let cases: &[(&str, f32, Vec<Rect>)] = &[
        (
            "default_budget_stays_split",
            DEFAULT_PASS_BUDGET_PX,
            rs.to_vec(),
        ),
        ("tight_budget_stays_split", 7_000.0, rs.to_vec()),
        ("high_budget_collapses", 60_000.0, vec![bbox]),
    ];
    for (label, budget_px, want) in cases {
        let mut region = Fold::new(*budget_px);
        for r in rs {
            region.add(r);
        }
        assert_eq!(region.rects(), *want, "case: {label}");
    }
}

/// `total_area` sums per-rect areas without subtracting overlap; this disjoint
/// case is the full-repaint heuristic's contract.
#[test]
fn total_area_sums_disjoint_rects() {
    let mut region = Fold::new(0.0);
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 100.0, 20.0, 20.0));
    assert_eq!(region.region.total_area(), 100.0 + 400.0);
}
