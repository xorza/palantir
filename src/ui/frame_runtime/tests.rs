use std::time::Duration;

use glam::UVec2;

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

#[test]
fn frame_classification_covers_external_entry_facts() {
    let cases = [
        Case {
            label: "first frame",
            previous: false,
            display_changed: false,
            damage_baseline_valid: true,
            wake: WakeReasons::default(),
            repaint_requested: false,
            input_policy: InputPolicy::OnDelta,
            input_signal: InputSignal::None,
            close_requested: false,
            expected: FramePlan::FullRecord { force_full: true },
        },
        Case {
            label: "display change",
            previous: true,
            display_changed: true,
            damage_baseline_valid: true,
            wake: WakeReasons::default(),
            repaint_requested: false,
            input_policy: InputPolicy::OnDelta,
            input_signal: InputSignal::None,
            close_requested: false,
            expected: FramePlan::FullRecord { force_full: true },
        },
        Case {
            label: "invalid prior output",
            previous: true,
            display_changed: false,
            damage_baseline_valid: false,
            wake: WakeReasons::ANIM,
            repaint_requested: false,
            input_policy: InputPolicy::OnDelta,
            input_signal: InputSignal::None,
            close_requested: false,
            expected: FramePlan::FullRecord { force_full: true },
        },
        Case {
            label: "animation wake",
            previous: true,
            display_changed: false,
            damage_baseline_valid: true,
            wake: WakeReasons::ANIM,
            repaint_requested: false,
            input_policy: InputPolicy::OnDelta,
            input_signal: InputSignal::None,
            close_requested: false,
            expected: FramePlan::PaintOnly,
        },
        Case {
            label: "real wake",
            previous: true,
            display_changed: false,
            damage_baseline_valid: true,
            wake: WakeReasons::REAL,
            repaint_requested: false,
            input_policy: InputPolicy::OnDelta,
            input_signal: InputSignal::None,
            close_requested: false,
            expected: FramePlan::FullRecord { force_full: false },
        },
        Case {
            label: "coalesced real and animation wake",
            previous: true,
            display_changed: false,
            damage_baseline_valid: true,
            wake: WakeReasons::REAL.merge(WakeReasons::ANIM),
            repaint_requested: false,
            input_policy: InputPolicy::OnDelta,
            input_signal: InputSignal::None,
            close_requested: false,
            expected: FramePlan::FullRecord { force_full: false },
        },
        Case {
            label: "always input policy",
            previous: true,
            display_changed: false,
            damage_baseline_valid: true,
            wake: WakeReasons::ANIM,
            repaint_requested: false,
            input_policy: InputPolicy::Always,
            input_signal: InputSignal::Inert,
            close_requested: false,
            expected: FramePlan::FullRecord { force_full: false },
        },
        Case {
            label: "delta input policy",
            previous: true,
            display_changed: false,
            damage_baseline_valid: true,
            wake: WakeReasons::ANIM,
            repaint_requested: false,
            input_policy: InputPolicy::OnDelta,
            input_signal: InputSignal::Repaint,
            close_requested: false,
            expected: FramePlan::FullRecord { force_full: false },
        },
        Case {
            label: "close request",
            previous: true,
            display_changed: false,
            damage_baseline_valid: true,
            wake: WakeReasons::ANIM,
            repaint_requested: false,
            input_policy: InputPolicy::OnDelta,
            input_signal: InputSignal::None,
            close_requested: true,
            expected: FramePlan::FullRecord { force_full: false },
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
