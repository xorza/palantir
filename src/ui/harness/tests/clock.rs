//! The two clocks, and the step sizes that would silently clamp.

use crate::ui::harness::tests::support::{INSIDE, SURFACE, button, target};
use crate::ui::harness::*;

#[test]
fn one_clock_stamps_both_frames_and_input() {
    // Rules 6 and 7. Time is frozen unless advanced, so two clicks at one
    // point are always a double-click — and an `advance` is what
    // separates them, with or without a frame after it. A host reads its
    // own clock at both doors, and an event-driven one can idle for
    // seconds between frames.
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    harness.click_at(INSIDE);
    let first = harness.response_in(target(), button);
    assert!(first.left.clicked());
    assert!(!first.left.double_clicked(), "one click is not a double");

    harness.click_at(INSIDE);
    let second = harness.response_in(target(), button);
    assert!(
        second.left.double_clicked(),
        "with the clock frozen the second click is always a double",
    );

    // Advancing *and* framing separates the runs.
    harness.advance_past_double_click(button);
    harness.click_at(INSIDE);
    let third = harness.response_in(target(), button);
    assert!(third.left.clicked());
    assert!(
        !third.left.double_clicked(),
        "past DOUBLE_CLICK_WINDOW the run restarts",
    );

    // No frame between the advance and the click: the press is stamped
    // when it arrives, so the gap separates the runs on its own.
    harness.advance(DOUBLE_CLICK_WINDOW * 2);
    harness.click_at(INSIDE);
    let fourth = harness.response_in(target(), button);
    assert!(
        !fourth.left.double_clicked(),
        "an advance reaches input timing without a frame to publish it",
    );

    // `at` is the same clock, parked absolutely instead of stepped.
    let parked = harness.time + DOUBLE_CLICK_WINDOW * 2;
    harness.at(parked).frame(button);
    assert_eq!(harness.time, parked, "at parks the clock absolutely");
    harness.click_at(INSIDE);
    let fifth = harness.response_in(target(), button);
    assert!(fifth.left.clicked());
    assert!(
        !fifth.left.double_clicked(),
        "at + frame separates press runs the same way advance does",
    );
}

#[test]
fn advance_frames_rejects_a_step_that_would_be_clamped() {
    // Rule 8. Animation dt is clamped to MAX_ANIM_DT per frame, so a
    // larger step silently under-integrates instead of failing.
    let mut harness = UiHarness::new(SURFACE);
    harness.advance_frames(3, Duration::from_millis(16), button);
    assert_eq!(harness.time, Duration::from_millis(48));

    let clamped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        UiHarness::new(SURFACE).advance_frames(1, Duration::from_millis(500), button);
    }));
    assert!(clamped.is_err(), "an over-MAX_ANIM_DT step must panic");
}
