use crate::damage::region::{DAMAGE_RECT_CAP, DEFAULT_PASS_BUDGET_PX, DamageRegion, MergePolicy};
use crate::primitives::geometry::rect::Rect;

/// A region and the policy its case folds under (an argument of [`DamageRegion::add`]).
#[derive(Debug)]
struct Fold {
    region: DamageRegion,
    policy: MergePolicy,
}

impl Fold {
    fn new(budget_px: f32) -> Self {
        Self {
            region: DamageRegion::default(),
            policy: MergePolicy::with_budget(budget_px),
        }
    }

    fn default_budget() -> Self {
        Self::new(DEFAULT_PASS_BUDGET_PX)
    }

    fn add(&mut self, r: Rect) {
        self.region.add(r, self.policy);
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

/// At the cap, the ninth rect triggers the min-growth fallback, which must respect the cap. A slot the forced merge grows over stays its own while merging it wastes past the budget.
#[test]
fn min_growth_at_cap_holds_the_cap() {
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
    let grown = target.union(extra);
    let rects = region.rects();
    assert_eq!(rects.len(), DAMAGE_RECT_CAP);
    assert!(rects.contains(&grown), "missing grown bbox: {rects:?}");
    assert!(
        rects.contains(&newly_overlapped),
        "the overlapped slot went: {rects:?}"
    );
    assert!(grown.intersects(newly_overlapped));
    for filler in fillers {
        assert!(
            rects.contains(&filler),
            "missing filler {filler:?}: {rects:?}"
        );
    }
}

/// Four compact small rects, 50 apart, under the default budget of 5,000: each row of two wastes
/// 150 × 50 − 2 × 50 × 50 = 2,500 and merges, and the two rows would waste
/// 150 × 150 − 2 × 150 × 50 = 7,500 and stay apart. At 8,000 they are one.
#[test]
fn compact_cluster_of_four_merges_by_rows_at_default_budget() {
    let squares = [
        Rect::new(100.0, 100.0, 50.0, 50.0),
        Rect::new(200.0, 100.0, 50.0, 50.0),
        Rect::new(100.0, 200.0, 50.0, 50.0),
        Rect::new(200.0, 200.0, 50.0, 50.0),
    ];
    let fold = |budget_px| {
        let mut region = Fold::new(budget_px);
        for r in squares {
            region.add(r);
        }
        region.rects()
    };
    assert_eq!(
        fold(DEFAULT_PASS_BUDGET_PX),
        vec![
            Rect::new(100.0, 100.0, 150.0, 50.0),
            Rect::new(100.0, 200.0, 150.0, 50.0),
        ]
    );
    assert_eq!(fold(8_000.0), vec![Rect::new(100.0, 100.0, 150.0, 150.0)]);
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

/// Disjoint rects take `total_area`'s no-overlap path: the sum of their areas.
#[test]
fn total_area_sums_disjoint_rects() {
    let mut region = Fold::new(0.0);
    region.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    region.add(Rect::new(100.0, 100.0, 20.0, 20.0));
    assert_eq!(region.region.total_area(), 100.0 + 400.0);
}

/// A cap of 3 folds four far-apart rects into three: the fourth forces the merge of least growth,
/// with its nearest neighbor 10 px away, (0, 0) + 10² and (20, 0) + 10² to (0, 0) + 30 × 10. A cap
/// of 1 folds all four into their bounds.
#[test]
fn rect_cap_bounds_the_fold() {
    let rects = [
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Rect::new(500.0, 0.0, 10.0, 10.0),
        Rect::new(0.0, 500.0, 10.0, 10.0),
        Rect::new(20.0, 0.0, 10.0, 10.0),
    ];
    let fold = |rect_cap| {
        let mut region = DamageRegion::default();
        for r in rects {
            region.add(
                r,
                MergePolicy {
                    budget_px: 0.0,
                    rect_cap,
                },
            );
        }
        let mut out: Vec<Rect> = region.iter_rects().collect();
        out.sort_by(|a, b| {
            a.min
                .x
                .total_cmp(&b.min.x)
                .then(a.min.y.total_cmp(&b.min.y))
        });
        out
    };
    assert_eq!(
        fold(3),
        vec![
            Rect::new(0.0, 0.0, 30.0, 10.0),
            Rect::new(0.0, 500.0, 10.0, 10.0),
            Rect::new(500.0, 0.0, 10.0, 10.0),
        ]
    );
    assert_eq!(fold(1), vec![Rect::new(0.0, 0.0, 510.0, 510.0)]);
}

/// A crosshair's strips stay apart: their bounds of 100 × 100 add
/// 10,000 − 1,000 − 1,000 + 100 = 8,100 px², past a budget of 8,000 and under one of 8,200. A rect
/// inside another goes at any budget, and the area counts their shared 100 px² once.
#[test]
fn an_overlapping_pair_merges_only_when_it_pays() {
    let wide = Rect::new(0.0, 45.0, 100.0, 10.0);
    let tall = Rect::new(45.0, 0.0, 10.0, 100.0);
    let fold = |budget_px, rects: &[Rect]| {
        let mut region = DamageRegion::default();
        for r in rects {
            region.add(*r, MergePolicy::with_budget(budget_px));
        }
        region
    };
    let split = fold(8_000.0, &[wide, tall]);
    assert_eq!(split.iter_rects().count(), 2);
    assert_eq!(split.total_area(), 1_900.0);
    assert_eq!(
        fold(8_200.0, &[wide, tall])
            .iter_rects()
            .collect::<Vec<_>>(),
        [Rect::new(0.0, 0.0, 100.0, 100.0)]
    );
    let inside = Rect::new(50.0, 46.0, 4.0, 4.0);
    assert_eq!(
        fold(0.0, &[inside, wide]).iter_rects().collect::<Vec<_>>(),
        [wide]
    );
    assert_eq!(
        fold(0.0, &[wide, inside]).iter_rects().collect::<Vec<_>>(),
        [wide]
    );
}

/// Three rects sharing a pixel area are counted by strips: 10 × 10 twice, 5 px apart, share 5 × 10,
/// and a third inside their overlap adds none, 150 in all.
#[test]
fn total_area_counts_overlaps_once() {
    let mut region = DamageRegion::from_unmerged(&[
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Rect::new(5.0, 0.0, 10.0, 10.0),
        Rect::new(6.0, 2.0, 3.0, 3.0),
    ]);
    assert_eq!(region.total_area(), 150.0);
    region = DamageRegion::from_unmerged(&[Rect::new(0.0, 0.0, 10.0, 10.0)]);
    assert_eq!(region.total_area(), 100.0);
}
