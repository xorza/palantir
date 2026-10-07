//! The area ratio that decides partial against full.

use crate::damage::Damage;
use crate::damage::engine::DamageEngine;
use crate::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use crate::damage::tests::support::{BLUE, DISPLAY, RED, TEST_SURFACE, frame, one_frame};
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;

/// A single-leaf fill flip stays partial: far below the full-repaint threshold.
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
    assert_eq!(
        Damage::expect_partial(Damage::new(h.collapsed_damage())),
        r.into()
    );
}

/// Coverage is `sum(rect.area()) / surface_area`; strictly above `FULL_REPAINT_THRESHOLD` (0.7) is Full, else
/// Partial (`>`, so exactly at the threshold stays Partial). Inputs go through `collapse_from`, the only
/// constructor that seals `coverage`; `region()` builds unsealed expected values, which match as coverage is
/// excluded from `PartialEq`.
#[test]
fn damage_filter_threshold_cases() {
    use crate::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
    fn region(rects: &[Rect]) -> DamageRegion {
        DamageRegion::from_rects(rects)
    }
    // Adjacent halves: a perfectly adjacent pair has `union_excess = 0`, so each pair collapses into one rect whose
    // area is the input sum, making the strict `>` the thing under test whatever the SAH budget.
    const PAIR_BELOW: [Rect; 2] = [
        // Merges to Rect(0,0,70,100); total_area = 0.70 → stays Partial.
        Rect::new(0.0, 0.0, 35.0, 100.0),
        Rect::new(35.0, 0.0, 35.0, 100.0),
    ];
    const PAIR_ABOVE: [Rect; 2] = [
        // Merges to Rect(0,0,72,100); total_area = 0.72 → escalates Full.
        Rect::new(0.0, 0.0, 36.0, 100.0),
        Rect::new(36.0, 0.0, 36.0, 100.0),
    ];
    // Expected: which outcome and, for a partial, which rects (the `Damage` coverage can't be stated by literals).
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
        // No zero-area-surface case: `collapse_from` asserts `surface_area > EPS`.
    ];
    for (label, rects, surface, want) in cases {
        let collapsed = DamageRegion::collapse_from(rects, DEFAULT_PASS_BUDGET_PX, *surface);
        match (Damage::new(collapsed), want) {
            (damage, Some(want)) => {
                assert_eq!(Damage::expect_partial(damage), *want, "case: {label}");
            }
            (Some(Damage::Full), None) => {}
            (other, None) => panic!("case: {label}: expected Full, got {other:?}"),
        }
    }
}

#[test]
fn no_damage_means_skip() {
    let d = DamageEngine::default();
    // No damage rect → `Skip` (the backbuffer already holds the right pixels), distinct from `Full`.
    assert_eq!(
        Damage::new(DamageRegion::collapse_from(
            &d.raw_rects,
            DEFAULT_PASS_BUDGET_PX,
            TEST_SURFACE
        )),
        None,
    );
}
