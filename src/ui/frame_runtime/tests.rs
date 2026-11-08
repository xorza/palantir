use crate::primitives::reduction::{Reduction, Reductions};
use std::time::Duration;

use glam::UVec2;

use crate::common::time::MAX_ANIMATION_DT;
use crate::damage::DamageBase;
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::primitives::input::policy::{InputPolicy, InputSignal};
use crate::ui::frame_runtime::FrameClassifyInput;
use crate::ui::frame_runtime::FramePlan;
use crate::ui::frame_runtime::FrameRuntime;
use crate::ui::frame_runtime::RecordBaseline;
use crate::ui::frame_runtime::wake::WakeReasons;

#[derive(Clone, Copy, Debug)]
struct Case {
    label: &'static str,
    prev: bool,
    display_changed: bool,
    damage_base: DamageBase,
    wake: WakeReasons,
    repaint_requested: bool,
    input_policy: InputPolicy,
    input_signal: InputSignal,
    close_requested: bool,
    /// What the retained tree was recorded under, against `BASELINE` this frame.
    recorded: Option<RecordBaseline>,
    expected: FramePlan,
}

/// The baseline every case's frame runs under.
const BASELINE: RecordBaseline = RecordBaseline {
    theme_epoch: 3,
    font_epoch: 7,
    effect_epoch: 4,
    device_epoch: 1,
    reductions: Reductions::FULL,
};

/// A warm frame with only an animation wake pending paints without recording.
const QUIET: Case = Case {
    label: "quiet",
    prev: true,
    display_changed: false,
    damage_base: DamageBase::Prev,
    wake: WakeReasons::ANIMATION,
    repaint_requested: false,
    input_policy: InputPolicy::OnDelta,
    input_signal: InputSignal::None,
    close_requested: false,
    recorded: Some(BASELINE),
    expected: FramePlan::PaintOnly,
};

#[test]
fn frame_classification_covers_external_entry_facts() {
    let cases = [
        Case {
            label: "first frame",
            prev: false,
            wake: WakeReasons::default(),
            expected: FramePlan::FullRecord {
                base: DamageBase::Discarded,
            },
            ..QUIET
        },
        Case {
            label: "display change",
            display_changed: true,
            wake: WakeReasons::default(),
            expected: FramePlan::FullRecord {
                base: DamageBase::Discarded,
            },
            ..QUIET
        },
        Case {
            label: "invalid prior output",
            damage_base: DamageBase::Discarded,
            expected: FramePlan::FullRecord {
                base: DamageBase::Discarded,
            },
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
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "coalesced real and animation wake",
            wake: WakeReasons::REAL.merge(WakeReasons::ANIMATION),
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "always input policy",
            input_policy: InputPolicy::Always,
            input_signal: InputSignal::Inert,
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "delta input policy",
            input_signal: InputSignal::Repaint,
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "theme swapped since the record",
            recorded: Some(RecordBaseline {
                theme_epoch: 2,
                ..BASELINE
            }),
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "font loaded since the record",
            recorded: Some(RecordBaseline {
                font_epoch: 6,
                ..BASELINE
            }),
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "effect loaded since the record",
            recorded: Some(RecordBaseline {
                effect_epoch: 3,
                ..BASELINE
            }),
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "transparency reduced since the record",
            recorded: Some(RecordBaseline {
                reductions: Reductions {
                    transparency: Reduction::Reduced,
                    ..Reductions::FULL
                },
                ..BASELINE
            }),
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "power reduced since the record",
            recorded: Some(RecordBaseline {
                reductions: Reductions {
                    power: Reduction::Reduced,
                    ..Reductions::FULL
                },
                ..BASELINE
            }),
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "motion reduced since the record",
            recorded: Some(RecordBaseline {
                reductions: Reductions {
                    motion: Reduction::Reduced,
                    ..Reductions::FULL
                },
                ..BASELINE
            }),
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "device attached since the record",
            recorded: Some(RecordBaseline {
                device_epoch: 0,
                ..BASELINE
            }),
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
            ..QUIET
        },
        Case {
            label: "close request",
            close_requested: true,
            expected: FramePlan::FullRecord {
                base: DamageBase::Prev,
            },
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
            stamp: FrameStamp::new(Display::default(), Duration::from_millis(10)),
            prev_stamp: case
                .prev
                .then_some(FrameStamp::new(base_display, Duration::ZERO)),
            repaint_requested: case.repaint_requested,
            recorded: case.recorded,
            ..FrameRuntime::default()
        };
        if case.wake != WakeReasons::default() {
            runtime.schedule_wake(Duration::from_millis(10), case.wake, None);
        }

        let actual = runtime.take_frame_plan(FrameClassifyInput {
            display,
            record_baseline: BASELINE,
            damage_base: case.damage_base,
            input_policy: case.input_policy,
            input_signal: case.input_signal,
            close_requested: case.close_requested,
        });

        assert_eq!(actual, case.expected, "{}", case.label);
        if actual != FramePlan::PaintOnly {
            assert_eq!(
                runtime.recorded,
                Some(BASELINE),
                "{}: a record adopts the baseline",
                case.label
            );
        }
    }
}

/// A frame's dt is the wall delta since the last, clamped at `MAX_ANIMATION_DT`: 1 ms is 1 ms, a
/// 100 ms stall clamps to 100 ms, and every frame of a 240 Hz panel and of a 10 µs loop spends its
/// own delta, where a 1/240 s accumulator spent nothing on every other one.
#[test]
fn a_frame_spends_its_wall_delta_inside_the_animation_bound() {
    let mut rt = FrameRuntime::default();
    rt.advance_clock(Duration::from_millis(1));
    assert_eq!(rt.dt, Duration::from_millis(1).as_secs_f32());
    rt.advance_clock(Duration::from_millis(201));
    assert_eq!(rt.dt, MAX_ANIMATION_DT, "a stall clamps");
    rt.advance_clock(Duration::from_millis(217));
    assert_eq!(rt.dt, Duration::from_millis(16).as_secs_f32());

    for step in [Duration::from_nanos(4_166_666), Duration::from_micros(10)] {
        let mut rt = FrameRuntime::default();
        let mut now = Duration::ZERO;
        for frame in 1..=8 {
            now += step;
            rt.advance_clock(now);
            assert_eq!(rt.dt, step.as_secs_f32(), "{step:?}: frame {frame}");
        }
    }
}

/// Wakes past the cap fold into a neighbour that fires no later than asked: at 64 distinct
/// deadlines a 65th, between two, pulls the later one forward to it, and one past them all
/// rides the last. None fires late, and the count stays at the cap.
#[test]
fn wakes_past_the_cap_fold_without_firing_late() {
    use crate::ui::frame_runtime::MAX_WAKES;
    let mut runtime = FrameRuntime::default();
    let ms = |n: u64| Duration::from_millis(n);
    for n in 0..MAX_WAKES as u64 {
        runtime.schedule_wake(ms(100 * (n + 1)), WakeReasons::REAL, None);
    }
    assert_eq!(runtime.repaint_wakes.len(), MAX_WAKES);
    runtime.schedule_wake(ms(150), WakeReasons::REAL, None);
    assert_eq!(runtime.repaint_wakes.len(), MAX_WAKES);
    assert_eq!(runtime.repaint_wakes[1].deadline, ms(150), "pulled forward");
    runtime.schedule_wake(ms(100_000), WakeReasons::REAL, None);
    assert_eq!(runtime.repaint_wakes.len(), MAX_WAKES);
    assert_eq!(
        runtime.repaint_wakes[MAX_WAKES - 1].deadline,
        ms(100 * MAX_WAKES as u64),
        "rides the last, which fires before it"
    );
}

/// The reduced-power grid at 60 Hz, interval 33_333_333 ns and slack 8_333_333 ns: a first frame
/// at 10 ms holds the next slot an interval on. A frame at the slot, or a slack before it, takes
/// it, so the next slot is two intervals from the first frame. A frame 1 ns sooner than that, or
/// one past the slot, starts the grid again at its own time. Full power holds no slot.
#[test]
fn a_frame_takes_its_reduced_power_slot_within_the_slack() {
    let ns = Duration::from_nanos;
    let interval = ns(33_333_333);
    let slack = ns(8_333_333);
    let display = Display {
        refresh_millihertz: Some(60_000),
        ..Display::default()
    };
    let first = Duration::from_millis(10);
    let slot = first + interval;
    let at = |held: Option<Duration>, time: Duration, power: Reduction| {
        let mut runtime = FrameRuntime {
            stamp: FrameStamp::new(display, time),
            reductions: Reductions {
                power,
                ..Reductions::FULL
            },
            earliest_repaint_at: held,
            ..FrameRuntime::default()
        };
        runtime.take_power_slot();
        runtime.earliest_repaint_at
    };
    for (label, held, time, want) in [
        ("first", None, first, Some(slot)),
        ("at the slot", Some(slot), slot, Some(slot + interval)),
        (
            "a slack early",
            Some(slot),
            slot - slack,
            Some(slot + interval),
        ),
        (
            "past the slack",
            Some(slot),
            slot - slack - ns(1),
            Some(slot - slack - ns(1) + interval),
        ),
        (
            "late",
            Some(slot),
            slot + ns(1),
            Some(slot + ns(1) + interval),
        ),
    ] {
        assert_eq!(at(held, time, Reduction::Reduced), want, "{label}");
    }
    assert_eq!(at(Some(slot), slot, Reduction::Full), None, "full power");
}
