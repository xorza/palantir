//! The two clocks, and the step sizes that would silently clamp.

use crate::internals::harness::tests::support::{INSIDE, SURFACE, button, target};
use crate::internals::harness::*;
use crate::internals::panic_probe;
use std::cell;

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

    harness.advance_past_double_click();
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

    panic_probe::assert_panics_with("exceeds MAX_ANIM_DT", || {
        UiHarness::new(SURFACE).advance_frames(1, Duration::from_millis(500), button);
    });
    panic_probe::assert_panics_with("exceeds MAX_ANIM_DT", || {
        UiHarness::new(SURFACE).frames_until_idle(1, Duration::from_millis(500), button);
    });
}

#[test]
fn frames_until_idle_counts_the_frames_a_motion_takes() {
    use crate::animation::anim_slot::AnimSlot;
    use crate::animation::anim_spec::AnimSpec;
    use crate::animation::easing::Easing;

    // A 50 ms linear tween from rest: the retarget frame spends nothing,
    // then 16, 32 and 48 ms are in flight and 64 ms passes the end, so
    // the fourth frame after it is idle.
    let slot = AnimSlot::new("idle-count");
    let tween = Some(AnimSpec::duration(0.05, Easing::Linear));
    let mut harness = UiHarness::new(SURFACE);
    let to = cell::Cell::new(0.0_f32);
    let mut record = |ui: &mut Ui| {
        let _ = ui.animate(target(), slot, to.get(), tween);
        button(ui);
    };
    harness.frame(&mut record);
    to.set(1.0);
    harness.frame(&mut record);
    let tick = Duration::from_millis(16);
    assert_eq!(harness.frames_until_idle(10, tick, &mut record), Some(4));
    assert_eq!(harness.time, tick * 4);

    // Still moving when `max` runs out reports none.
    to.set(0.0);
    harness.frame(&mut record);
    assert_eq!(harness.frames_until_idle(2, tick, &mut record), None);
}
