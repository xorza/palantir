//! The two clocks, and the step sizes that would silently clamp.

use crate::host::harness::tests::support::{self, INSIDE, SURFACE};
use crate::host::harness::*;
use crate::input_state::capture::DOUBLE_CLICK_WINDOW;
use crate::internals::panic_probe;
use crate::ui::resources::UiResources;
use std::cell;

#[test]
fn one_clock_stamps_both_frames_and_input() {
    // Time is frozen unless advanced, so two clicks at one point are a double-click until `advance` separates them.
    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, support::button);

    harness.click(INSIDE);
    let first = harness.response_in(support::target(), support::button);
    assert!(first.left.clicked());
    assert!(!first.left.double_clicked(), "one click is not a double");

    harness.click(INSIDE);
    let second = harness.response_in(support::target(), support::button);
    assert!(
        second.left.double_clicked(),
        "with the clock frozen the second click is always a double",
    );

    harness.advance_past_double_click();
    harness.click(INSIDE);
    let third = harness.response_in(support::target(), support::button);
    assert!(third.left.clicked());
    assert!(
        !third.left.double_clicked(),
        "past DOUBLE_CLICK_WINDOW the run restarts",
    );

    harness.advance(DOUBLE_CLICK_WINDOW * 2);
    harness.click(INSIDE);
    let fourth = harness.response_in(support::target(), support::button);
    assert!(
        !fourth.left.double_clicked(),
        "an advance reaches input timing without a frame to publish it",
    );

    let parked = harness.time + DOUBLE_CLICK_WINDOW * 2;
    harness.at(parked).frame(support::button);
    assert_eq!(harness.time, parked, "at parks the clock absolutely");
    harness.click(INSIDE);
    let fifth = harness.response_in(support::target(), support::button);
    assert!(fifth.left.clicked());
    assert!(
        !fifth.left.double_clicked(),
        "at + frame separates press runs the same way advance does",
    );
}

#[test]
fn advance_frames_rejects_a_step_that_would_be_clamped() {
    // Animation dt clamps to MAX_ANIMATION_DT per frame; a larger step silently under-integrates.
    let mut harness = Harness::mono(SURFACE);
    harness.advance_frames(3, Duration::from_millis(16), support::button);
    assert_eq!(harness.time, Duration::from_millis(48));

    panic_probe::assert_panics_with("an animation integrates", || {
        Harness::mono(SURFACE).advance_frames(1, Duration::from_millis(500), support::button);
    });
    panic_probe::assert_panics_with("an animation integrates", || {
        Harness::builder(SURFACE)
            .frame_dt(Duration::from_millis(500))
            .build()
    });
}

#[test]
fn settle_counts_the_frames_a_motion_takes() {
    use crate::animation::animation_request::AnimationRequest;
    use crate::animation::animation_slot::AnimationSlot;
    use crate::animation::animation_spec::AnimationSpec;
    use crate::animation::easing::Easing;

    // 50 ms linear tween: the retarget frame spends nothing, 16/32/48 ms are in flight, 64 ms passes the end.
    let slot = AnimationSlot::new("idle-count");
    let tween = Some(AnimationSpec::duration(
        Duration::from_millis(50),
        Easing::Linear,
    ));
    let tick = Duration::from_millis(16);
    let mut harness = Harness::builder(SURFACE).mono().frame_dt(tick).build();
    let to = cell::Cell::new(0.0_f32);
    let mut record = |ui: &mut Ui| {
        let _ = ui.animate(
            AnimationRequest::new(support::target(), slot, to.get()),
            tween,
        );
        support::button(ui);
    };
    harness.frame(&mut record);
    to.set(1.0);
    harness.frame(&mut record);
    assert_eq!(harness.settle(&mut record), Ok(4));
    assert_eq!(harness.time, tick * 4);
}

/// A host's double-click time reaches every window that shares its resources, and each press
/// reads it: two clicks 300 ms apart are a double click under the 500 ms default and two clicks
/// under a 200 ms platform setting, the one the winit host would set.
#[test]
fn every_window_reads_the_hosts_double_click_time() {
    let gap = Duration::from_millis(300);
    let double_after_gap = |harness: &mut Harness| {
        harness.prime(2, support::button);
        harness.click(INSIDE);
        harness.frame(support::button);
        harness.advance(gap);
        harness.click(INSIDE);
        harness
            .response_in(support::target(), support::button)
            .left
            .double_clicked()
    };

    let mut stock = Harness::mono(SURFACE);
    assert_eq!(stock.ui().double_click_window(), DOUBLE_CLICK_WINDOW);
    assert!(double_after_gap(&mut stock), "300 ms is inside 500 ms");

    let shared = UiResources::isolated_mono();
    let mut first = Harness::from_resources(shared.clone(), SURFACE);
    let mut second = Harness::from_resources(shared.clone(), SURFACE);
    shared.double_click_window().set(Duration::from_millis(200));
    for harness in [&mut first, &mut second] {
        assert_eq!(
            harness.ui().double_click_window(),
            Duration::from_millis(200)
        );
    }
    assert!(!double_after_gap(&mut first), "300 ms is past 200 ms");
}
