use std::time::Duration;

use glam::UVec2;

use crate::common::time::MAX_ANIM_DT;
use crate::display::Display;
use crate::input::policy::{InputPolicy, InputSignal};
use crate::ui::frame_runtime::FrameClassifyInput;
use crate::ui::frame_runtime::FramePlan;
use crate::ui::frame_runtime::FrameRuntime;
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::frame_stamp::FrameStamp;

#[derive(Clone, Copy, Debug)]
struct Case {
    label: &'static str,
    previous: bool,
    display_changed: bool,
    damage_baseline_valid: bool,
    wake: WakeReasons,
    repaint_requested: bool,
    input_policy: InputPolicy,
    input_signal: InputSignal,
    close_requested: bool,
    expected: FramePlan,
}

/// A warm frame with only an animation wake pending paints without recording.
const QUIET: Case = Case {
    label: "quiet",
    previous: true,
    display_changed: false,
    damage_baseline_valid: true,
    wake: WakeReasons::ANIM,
    repaint_requested: false,
    input_policy: InputPolicy::OnDelta,
    input_signal: InputSignal::None,
    close_requested: false,
    expected: FramePlan::PaintOnly,
};

#[test]
fn frame_classification_covers_external_entry_facts() {
    let cases = [
        Case {
            label: "first frame",
            previous: false,
            wake: WakeReasons::default(),
            expected: FramePlan::FullRecord { force_full: true },
            ..QUIET
        },
        Case {
            label: "display change",
            display_changed: true,
            wake: WakeReasons::default(),
            expected: FramePlan::FullRecord { force_full: true },
            ..QUIET
        },
        Case {
            label: "invalid prior output",
            damage_baseline_valid: false,
            expected: FramePlan::FullRecord { force_full: true },
            ..QUIET
        },
        Case {
            label: "animation wake",
            expected: FramePlan::PaintOnly,
            ..QUIET
        },
        Case {
            label: "real wake",
            wake: WakeReasons::REAL,
            expected: FramePlan::FullRecord { force_full: false },
            ..QUIET
        },
        Case {
            label: "coalesced real and animation wake",
            wake: WakeReasons::REAL.merge(WakeReasons::ANIM),
            expected: FramePlan::FullRecord { force_full: false },
            ..QUIET
        },
        Case {
            label: "always input policy",
            input_policy: InputPolicy::Always,
            input_signal: InputSignal::Inert,
            expected: FramePlan::FullRecord { force_full: false },
            ..QUIET
        },
        Case {
            label: "delta input policy",
            input_signal: InputSignal::Repaint,
            expected: FramePlan::FullRecord { force_full: false },
            ..QUIET
        },
        Case {
            label: "close request",
            close_requested: true,
            expected: FramePlan::FullRecord { force_full: false },
            ..QUIET
        },
    ];

    let base_display = Display::from_physical(UVec2::new(100, 80), 1.0);
    for case in cases {
        let display = if case.display_changed {
            Display::from_physical(UVec2::new(101, 80), 1.0)
        } else {
            base_display
        };
        let mut runtime = FrameRuntime {
            time: Duration::from_millis(10),
            prev_stamp: case
                .previous
                .then_some(FrameStamp::new(base_display, Duration::ZERO)),
            repaint_requested: case.repaint_requested,
            ..FrameRuntime::default()
        };
        if case.wake != WakeReasons::default() {
            runtime.schedule_wake(Duration::from_millis(10), case.wake, None);
        }

        let actual = runtime.take_frame_plan(FrameClassifyInput {
            display,
            damage_baseline_valid: case.damage_baseline_valid,
            input_policy: case.input_policy,
            input_signal: case.input_signal,
            close_requested: case.close_requested,
        });

        assert_eq!(actual, case.expected, "{}", case.label);
    }
}

/// A frame's spend is bounded: the accumulator carries unspent time over the clamped wall delta, and `spring::step` takes `MAX_ANIM_DT` as a contract.
///
/// One short frame, then a stall: 1 ms carries, 100 ms clamps, 101 ms is the unbounded sum.
#[test]
fn a_spent_delta_stays_inside_the_animation_bound() {
    let mut rt = FrameRuntime::default();

    rt.advance_clock(Duration::from_millis(1));
    assert_eq!(
        rt.dt, 0.0,
        "a frame under the accumulator step spends nothing"
    );
    assert_eq!(
        rt.dt_accum,
        Duration::from_millis(1).as_secs_f32(),
        "and carries it whole"
    );

    rt.advance_clock(Duration::from_millis(101));
    assert_eq!(
        rt.dt, MAX_ANIM_DT,
        "the carry may not push a spent delta past the bound",
    );
    assert_eq!(rt.dt_accum, 0.0, "a spending frame leaves nothing behind");

    rt.advance_clock(Duration::from_millis(117));
    assert_eq!(
        rt.dt,
        Duration::from_millis(16).as_secs_f32(),
        "a 16 ms frame spends 16 ms"
    );

    // At 10 µs a frame nothing is spent until the carry crosses 1/240 s = 4.1667 ms: 416 frames carry 4.16 ms; the 417th spends 4.17 ms.
    let mut rt = FrameRuntime::default();
    let mut now = Duration::ZERO;
    for frame in 1..=416 {
        now += Duration::from_micros(10);
        rt.advance_clock(now);
        assert_eq!(rt.dt, 0.0, "frame {frame} spends nothing");
    }
    now += Duration::from_micros(10);
    rt.advance_clock(now);
    let carried = (0..417).fold(0.0f32, |sum, _| {
        sum + Duration::from_micros(10).as_secs_f32()
    });
    assert_eq!(rt.dt, carried, "frame 417 spends the whole carry");
}
