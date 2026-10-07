//! The two clocks, and the step sizes that would silently clamp.

use crate::internals::harness::tests::support::{INSIDE, SURFACE, button, target};
use crate::internals::harness::*;
use crate::internals::panic_probe;
use std::cell;

#[test]
fn one_clock_stamps_both_frames_and_input() {
    // Time is frozen unless advanced, so two clicks at one point are a double-click until `advance` separates them.
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

    harness.advance(DOUBLE_CLICK_WINDOW * 2);
    harness.click_at(INSIDE);
    let fourth = harness.response_in(target(), button);
    assert!(
        !fourth.left.double_clicked(),
        "an advance reaches input timing without a frame to publish it",
    );

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
    // Animation dt clamps to MAX_ANIM_DT per frame; a larger step silently under-integrates.
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
    use crate::animation::animation_slot::AnimationSlot;
    use crate::animation::animation_spec::AnimationSpec;
    use crate::animation::easing::Easing;

    // 50 ms linear tween: the retarget frame spends nothing, 16/32/48 ms are in flight, 64 ms passes the end.
    let slot = AnimationSlot::new("idle-count");
    let tween = Some(AnimationSpec::duration(
        Duration::from_millis(50),
        Easing::Linear,
    ));
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

    to.set(0.0);
    harness.frame(&mut record);
    assert_eq!(harness.frames_until_idle(2, tick, &mut record), None);
}
