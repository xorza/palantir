//! The area ratio that decides partial against full.

use crate::primitives::rect::Rect;
use crate::scene::damage::region::DamageRegion;
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, TEST_SURFACE, frame, one_frame};
use crate::scene::damage::{Damage, DamageEngine};
use crate::ui::harness::UiHarness;

/// Pin: a single-leaf fill flip stays in the partial-repaint regime —
/// `filter(surface)` returns `Partial(rect)`, because the rect is well
/// below the full-repaint threshold (50×50 = 2500 ≪ 200×200 surface).
#[test]
fn damage_filter_returns_partial_when_small() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut h, |ui| {
        one_frame(ui, RED);
    });
    let region = h.damage_region();
    let r = region
        .iter_rects()
        .next()
        .expect("single-leaf change → some damage");
    assert_eq!(Damage::new(h.collapsed_damage()).expect_partial(), r.into());
}

/// Heuristic: total coverage = `sum(rect.area()) / surface_area`;
/// strictly above `FULL_REPAINT_THRESHOLD` (0.7) ⇒ Full, otherwise
/// Partial. The check is `>`, not `>=`, so coverage exactly at the
/// threshold stays Partial. `total_area` sums per-rect areas of the
/// post-merge region, so adjacent rects that the proximity-merge
/// rule collapses contribute their merged-bbox area (which here
/// equals the input sum since they tile cleanly). Inputs go through
/// `collapse_from` (the only constructor that seals `coverage`); the
/// `region()` helper builds the unsealed *expected* values, which
/// still match because coverage is excluded from `PartialEq`.
#[test]
fn damage_filter_threshold_cases() {
    use crate::scene::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
    fn region(rects: &[Rect]) -> DamageRegion {
        DamageRegion::from_rects(rects)
    }
    // Adjacent halves on the 100×100 surface — a perfectly adjacent
    // pair has `union_excess = bbox − a − b = 0`, below any positive
    // SAH budget, so each pair collapses into one rect whose area
    // equals the input sum. The region's `total_area` then lands
    // exactly at the threshold (or just above) and the strict `>`
    // decision logic is what's under test; the merge is guaranteed by
    // the zero excess alone, independent of the budget's exact value.
    const PAIR_BELOW: [Rect; 2] = [
        // Merges to Rect(0,0,70,100); total_area = 7000 / 10000 = 0.70
        // → stays Partial (`>` is strict).
        Rect::new(0.0, 0.0, 35.0, 100.0),
        Rect::new(35.0, 0.0, 35.0, 100.0),
    ];
    const PAIR_ABOVE: [Rect; 2] = [
        // Merges to Rect(0,0,72,100); total_area = 7200 / 10000 = 0.72
        // → escalates Full.
        Rect::new(0.0, 0.0, 36.0, 100.0),
        Rect::new(36.0, 0.0, 36.0, 100.0),
    ];
    // Expected damage as "which outcome, and — for a partial — which
    // rects". The whole `Damage` is not the comparison: it carries the
    // coverage the frame measured, which these rect literals have no way
    // to state.
    let cases: &[(&str, &[Rect], Rect, Option<DamageRegion>)] = &[
        (
            "small_1pct",
            &[Rect::new(0.0, 0.0, 10.0, 10.0)],
            TEST_SURFACE,
            Some(Rect::new(0.0, 0.0, 10.0, 10.0).into()),
        ),
        (
            "large_81pct_above_threshold",
            &[Rect::new(0.0, 0.0, 90.0, 90.0)],
            TEST_SURFACE,
            None,
        ),
        (
            "below_threshold_64pct_stays_partial",
            &[Rect::new(0.0, 0.0, 80.0, 80.0)],
            TEST_SURFACE,
            Some(Rect::new(0.0, 0.0, 80.0, 80.0).into()),
        ),
        (
            "exact_70pct_stays_partial",
            &[Rect::new(0.0, 0.0, 70.0, 100.0)],
            TEST_SURFACE,
            Some(Rect::new(0.0, 0.0, 70.0, 100.0).into()),
        ),
        (
            "two_rect_sum_at_threshold_stays_partial",
            &PAIR_BELOW,
            TEST_SURFACE,
            Some(region(&PAIR_BELOW)),
        ),
        (
            "two_rect_sum_above_threshold_escalates_full",
            &PAIR_ABOVE,
            TEST_SURFACE,
            None,
        ),
        // Zero-area-surface case dropped: `collapse_from` now asserts
        // `surface_area > EPS` (host filters resize-to-zero before we
        // ever reach this layer), so the prior `Damage::Full` fallback
        // became unreachable.
    ];
    for (label, rects, surface, want) in cases {
        let collapsed = DamageRegion::collapse_from(rects, DEFAULT_PASS_BUDGET_PX, *surface);
        match (Damage::new(collapsed), want) {
            (damage, Some(want)) => {
                assert_eq!(damage.expect_partial(), *want, "case: {label}")
            }
            (Damage::Full, None) => {}
            (other, None) => panic!("case: {label}: expected Full, got {other:?}"),
        }
    }
}

#[test]
fn no_damage_means_skip() {
    let d = DamageEngine::default();
    // No damage rect → `filter` returns `Skip` (no work to do; the
    // backbuffer already holds the right pixels). Distinct from
    // `Full` ("everything changed"), which is what coverage above
    // [`FULL_REPAINT_THRESHOLD`] produces.
    assert_eq!(
        Damage::new(DamageRegion::collapse_from(
            &d.raw_rects,
            d.budget_px,
            TEST_SURFACE
        )),
        Damage::Skip,
    );
}
