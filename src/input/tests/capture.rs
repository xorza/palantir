//! The press-run rule on its own: whether a press chains onto the last one, at the interval's and the radius's edges.

use crate::input::capture::{Capture, DOUBLE_CLICK_INTERVAL, DOUBLE_CLICK_RADIUS, PressRun};
use crate::primitives::identity::widget_id::WidgetId;
use glam::Vec2;
use std::time::Duration;

fn target() -> WidgetId {
    WidgetId::from_hash("target")
}

/// Each row presses once after a run of `count` presses on `target` at the origin at time zero,
/// under the double-click `interval`. The interval is the press's to read, as a platform setting
/// moves it: 300 ms after the last chains under a 400 ms interval, as GNOME's default, and starts
/// over under a 200 ms one.
#[test]
fn a_press_chains_inside_the_window_and_radius_on_the_same_target() {
    #[derive(Debug)]
    struct Row {
        label: &'static str,
        count: u8,
        on: WidgetId,
        pos: Vec2,
        now: Duration,
        interval: Duration,
        want: u8,
    }
    let row = |label, count, pos, now, want| Row {
        label,
        count,
        on: target(),
        pos,
        now,
        interval: DOUBLE_CLICK_INTERVAL,
        want,
    };
    let ms = Duration::from_millis;
    let rows = [
        row(
            "a quick press on the spot chains",
            1,
            Vec2::ZERO,
            Duration::ZERO,
            2,
        ),
        row(
            "exactly the interval still chains",
            1,
            Vec2::ZERO,
            DOUBLE_CLICK_INTERVAL,
            2,
        ),
        row(
            "a nanosecond past the interval starts over",
            1,
            Vec2::ZERO,
            DOUBLE_CLICK_INTERVAL + Duration::from_nanos(1),
            1,
        ),
        row(
            "exactly the radius still chains",
            2,
            Vec2::new(DOUBLE_CLICK_RADIUS, 0.0),
            Duration::ZERO,
            3,
        ),
        row(
            "past the radius starts over",
            2,
            Vec2::new(DOUBLE_CLICK_RADIUS + 0.01, 0.0),
            Duration::ZERO,
            1,
        ),
        Row {
            on: WidgetId::from_hash("other"),
            ..row(
                "another target starts over",
                1,
                Vec2::ZERO,
                Duration::ZERO,
                1,
            )
        },
        row(
            "the count saturates",
            u8::MAX,
            Vec2::ZERO,
            Duration::ZERO,
            u8::MAX,
        ),
        Row {
            interval: ms(400),
            ..row(
                "a 400 ms interval holds a 300 ms gap",
                1,
                Vec2::ZERO,
                ms(300),
                2,
            )
        },
        Row {
            interval: ms(200),
            ..row("a 200 ms interval does not", 1, Vec2::ZERO, ms(300), 1)
        },
    ];
    for Row {
        label,
        count,
        on,
        pos,
        now,
        interval,
        want,
    } in rows
    {
        let mut cap = Capture::after(PressRun::new(Duration::ZERO, target(), Vec2::ZERO, count));
        cap.begin_press(on, pos, now, interval);
        assert_eq!(cap.press_count(), Some(want), "{label}");
        assert_eq!(
            cap.run(),
            Some(PressRun::new(now, on, pos, want)),
            "{label}: the run follows the press"
        );
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
        DOUBLE_CLICK_INTERVAL,
    );
    assert_eq!(cap.press_count(), Some(1));
}
