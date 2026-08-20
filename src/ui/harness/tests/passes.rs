//! Which record pass a read observes, warm against cold.

use crate::ui::frame_report::{FramePaint, FrameProcessing};
use crate::ui::harness::tests::support::{INSIDE, SURFACE, button, target};
use crate::ui::harness::*;

#[test]
fn warm_constructors_run_one_pass_and_cold_runs_two() {
    // Rule 1. `cold` leaves `prev_stamp` unseeded so frame 1 adds the
    // blackout warmup pass; every other constructor seeds it. The count
    // is the contract — `FrameProcessing` cannot report the warmup, so a
    // test that got this wrong would silently read the input-blind pass.
    let mut passes = PassCounter::default();

    let mut warm = UiHarness::new(SURFACE);
    let report = warm.frame(|ui| {
        passes.0 += 1;
        button(ui);
    });
    assert_eq!(passes.take(), 1, "a warm frame 1 records once");
    assert_eq!(report.processing, FrameProcessing::SingleLayout);

    let mut cold = UiHarness::cold(SURFACE);
    cold.frame(|ui| {
        passes.0 += 1;
        button(ui);
    });
    assert_eq!(passes.take(), 2, "a cold frame 1 records warmup + pass A");

    // Frame 2 is single either way — the warmup is a frame-1 event only.
    cold.frame(|ui| {
        passes.0 += 1;
        button(ui);
    });
    assert_eq!(passes.take(), 1);
}

#[test]
fn frame_value_returns_pass_a_not_the_drained_second_pass() {
    // Rules 4 and 5. A click makes the frame double-record; pass B runs
    // after `drain_per_frame_queues`, so it sees `clicked() == false`.
    // Reading the last pass — the `let mut x = …; frame(|ui| x = …)`
    // shape — silently loses the edge.
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    harness.click_at(INSIDE);

    // Instrumenting the raw `frame` closure, which runs on every pass —
    // this is what a caller writing `let mut x = …; frame(|ui| x = …)`
    // would end up reading.
    let mut per_pass = Vec::new();
    let report = harness.frame(|ui| {
        button(ui);
        per_pass.push(ui.response_for(target()).left.clicked());
    });

    assert_eq!(report.processing, FrameProcessing::DoubleLayout);
    assert_eq!(
        per_pass,
        vec![true, false],
        "the click frame records twice and only pass A sees the edge",
    );

    // Same click, through `frame_value`: the scene still records on both
    // passes, but the value comes from the one that saw the edge.
    harness.click_at(INSIDE);
    let mut passes = 0;
    let clicked = harness.frame_value(|ui| {
        button(ui);
        passes += 1;
        ui.response_for(target()).left.clicked()
    });

    assert_eq!(passes, 2, "frame_value must not skip pass B's recording");
    assert!(clicked, "…but must return pass A's value");
}

#[test]
fn response_in_sees_the_click_that_a_between_frames_read_misses() {
    // Rule 3. `frame_quiescent` is snapshotted at record-pass start, so
    // a read taken between frames reflects the *previous* frame's input.
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    harness.click_at(INSIDE);

    let stale = harness.ui.response_for(target()).left.clicked();
    assert!(!stale, "a between-frames read cannot see input fed since");

    let inside = harness.response_in(target(), button);
    assert!(inside.left.clicked(), "the same click, read inside pass A");
}

#[test]
fn frame_without_baseline_forces_a_full_repaint() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    // A steady frame with no input has nothing to repaint.
    assert_eq!(harness.frame(button).paint(), FramePaint::Skip);
    // Dropping the baseline forces the whole surface.
    assert_eq!(
        harness.frame_without_baseline(button).paint(),
        FramePaint::Full
    );
}

/// Counts record-closure invocations per frame — the fact the whole
/// protocol follows from.
#[derive(Debug, Default)]
struct PassCounter(u32);

impl PassCounter {
    fn take(&mut self) -> u32 {
        std::mem::replace(&mut self.0, 0)
    }
}
