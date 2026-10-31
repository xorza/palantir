//! The press-run rule on its own: whether a press chains onto the last one, at the window's and the radius's edges.

use crate::input_state::capture::{Capture, DOUBLE_CLICK_RADIUS, DOUBLE_CLICK_WINDOW, PressRun};
use crate::primitives::identity::widget_id::WidgetId;
use glam::Vec2;
use std::time::Duration;

fn target() -> WidgetId {
    WidgetId::from_hash("target")
}

/// Each row presses once after a run of `count` presses on `target` at the origin at time zero.
#[test]
fn a_press_chains_inside_the_window_and_radius_on_the_same_target() {
    let other = WidgetId::from_hash("other");
    let rows: &[(&str, u8, WidgetId, Vec2, Duration, u8)] = &[
        (
            "a quick press on the spot chains",
            1,
            target(),
            Vec2::ZERO,
            Duration::ZERO,
            2,
        ),
        (
            "exactly the window still chains",
            1,
            target(),
            Vec2::ZERO,
            DOUBLE_CLICK_WINDOW,
            2,
        ),
        (
            "a nanosecond past the window starts over",
            1,
            target(),
            Vec2::ZERO,
            DOUBLE_CLICK_WINDOW + Duration::from_nanos(1),
            1,
        ),
        (
            "exactly the radius still chains",
            2,
            target(),
            Vec2::new(DOUBLE_CLICK_RADIUS, 0.0),
            Duration::ZERO,
            3,
        ),
        (
            "past the radius starts over",
            2,
            target(),
            Vec2::new(DOUBLE_CLICK_RADIUS + 0.01, 0.0),
            Duration::ZERO,
            1,
        ),
        (
            "another target starts over",
            1,
            other,
            Vec2::ZERO,
            Duration::ZERO,
            1,
        ),
        (
            "the count saturates",
            u8::MAX,
            target(),
            Vec2::ZERO,
            Duration::ZERO,
            u8::MAX,
        ),
    ];
    for &(label, count, on, pos, now, want) in rows {
        let mut cap = Capture {
            run: Some(PressRun {
                at: Duration::ZERO,
                target: target(),
                pos: Vec2::ZERO,
                count,
            }),
            ..Capture::default()
        };
        cap.begin_press(on, pos, now, DOUBLE_CLICK_WINDOW);
        let press = cap.press.expect("a press latched");
        assert_eq!(press.count, want, "{label}");
        let run = cap.run.expect("the run follows the press");
        assert_eq!((run.count, run.target, run.at), (want, on, now), "{label}");
    }
}

/// The window is the press's to read, as a platform setting moves it: a press 300 ms after the
/// last chains under a 400 ms window, as GNOME's default, and starts over under a 200 ms one.
#[test]
fn the_window_a_press_reads_decides_the_chain() {
    let gap = Duration::from_millis(300);
    for (window, want) in [
        (Duration::from_millis(400), 2),
        (Duration::from_millis(200), 1),
    ] {
        let mut cap = Capture {
            run: Some(PressRun {
                at: Duration::ZERO,
                target: target(),
                pos: Vec2::ZERO,
                count: 1,
            }),
            ..Capture::default()
        };
        cap.begin_press(target(), Vec2::ZERO, gap, window);
        assert_eq!(cap.press.map(|press| press.count), Some(want), "{window:?}");
    }
}

/// With no run, the first press is the first of a run.
#[test]
fn a_first_press_starts_a_run_at_one() {
    let mut cap = Capture::default();
    cap.begin_press(
        target(),
        Vec2::ONE,
        Duration::from_secs(3),
        DOUBLE_CLICK_WINDOW,
    );
    assert_eq!(cap.press.map(|press| press.count), Some(1));
}
