//! Reduced power, as the system's saver mode asks or a preference picks: one value a frame.

use std::time::Duration;

use crate::host::harness::Harness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::ui::Ui;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::tests::support::SURFACE;
use crate::ui::ui_preferences::UiPreferences;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;

fn pick(ui: &mut Ui, power: Preference<Reduction>) {
    ui.set_preferences(UiPreferences {
        power,
        ..ui.preferences()
    });
}

/// A pick made inside a frame leaves that frame's power as it started, and the next frame, a full
/// record, runs under it. The system's saver mode reaches a frame only through `System`: a pick
/// of full holds over it, and a return to `System` follows it.
#[test]
fn power_moves_only_between_frames_and_follows_the_system_through_system() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |_| {});
    assert!(!h.ui().is_power_reduced());

    let mut seen = None;
    let report = h.frame(|ui| {
        pick(ui, Preference::Fixed(Reduction::Reduced));
        seen = Some(ui.is_power_reduced());
    });
    assert_eq!(
        seen,
        Some(false),
        "the frame keeps the power it started with"
    );
    assert!(report.repaint_requested, "the move asks for a frame");
    let report = h.frame(|_| {});
    assert!(
        h.ui().is_power_reduced(),
        "the next frame runs under the pick"
    );
    assert_ne!(report.processing, FrameProcessing::PaintOnly);

    h.frame(|ui| pick(ui, Preference::Fixed(Reduction::Full)));
    h.set_system_power_reduced(true);
    h.frame(|_| {});
    assert!(h.ui().system_power_reduced());
    assert!(
        !h.ui().is_power_reduced(),
        "a pick of full over the saver mode"
    );

    h.frame(|ui| pick(ui, Preference::System));
    h.frame(|_| {});
    assert!(h.ui().is_power_reduced(), "System follows the saver mode");
}

fn reduced_at(millihertz: u32) -> Harness {
    Harness::builder(SURFACE)
        .mono()
        .refresh_millihertz(millihertz)
        .system_power_reduced(true)
        .build()
}

/// The time of each of `count` frames, on a clock the test does not move.
fn frame_times(h: &mut Harness, count: usize) -> Vec<Duration> {
    (0..count)
        .map(|_| {
            h.frame(|_| {});
            h.ui().now()
        })
        .collect()
}

/// While power is reduced, each frame starts a whole number of refresh periods at or above
/// 1/30 s after the last, though nothing moves the clock: 2 periods at 60 Hz, 4 at 120 Hz and 5
/// at 144 Hz, N × 1e12 / mHz ns truncated, so 33_333_333, 33_333_333 and 34_722_222. Each frame
/// reports the next slot. Full power runs every frame at once.
#[test]
fn reduced_power_spaces_frames_by_whole_refresh_periods() {
    let ns = Duration::from_nanos;
    for (millihertz, interval) in [
        (60_000, ns(33_333_333)),
        (120_000, ns(33_333_333)),
        (144_000, ns(34_722_222)),
    ] {
        let mut h = reduced_at(millihertz);
        assert_eq!(
            frame_times(&mut h, 3),
            [Duration::ZERO, interval, interval * 2],
            "{millihertz} mHz"
        );
        let report = h.frame(|_| {});
        assert_eq!(
            report.earliest_repaint_at,
            Some(interval * 4),
            "{millihertz} mHz"
        );
    }
    let mut h = Harness::builder(SURFACE)
        .mono()
        .refresh_millihertz(60_000)
        .build();
    assert_eq!(frame_times(&mut h, 3), [Duration::ZERO; 3]);
    assert_eq!(h.frame(|_| {}).earliest_repaint_at, None);
}

/// Input that arrives inside the interval paints at its end: a hover fed 10 ms after a frame at 0
/// lands in the frame at 2 periods of 60 Hz, 33_333_333 ns, which sees it. A frame asked for
/// after an idle 100 ms runs at once and starts the grid again from its own time.
#[test]
fn input_inside_the_interval_paints_at_its_end() {
    let id = WidgetId::from_hash("target");
    let mut hovered = |ui: &mut Ui| Button::new().id(id).label("target").show(ui).hovered();
    let mut h = reduced_at(60_000);
    h.frame(|ui| {
        hovered(ui);
    });
    assert_eq!(h.ui().now(), Duration::ZERO);

    h.advance(Duration::from_millis(10));
    h.hover("target");
    assert!(h.frame_value(&mut hovered), "the frame sees the hover");
    let slot = Duration::from_nanos(33_333_333);
    assert_eq!(h.ui().now(), slot, "the hover paints at the interval's end");

    h.advance(Duration::from_millis(100));
    let idle = slot + Duration::from_millis(100);
    assert_eq!(frame_times(&mut h, 2), [idle, idle + slot]);
}
