//! Pin tests for the animation primitive (generic over `Animatable`).
//! Covers first-touch, duration interpolation, retarget mid-flight,
//! spring convergence, settle clears repaint, removed-widget eviction,
//! plus typed-slot dispatch via `Vec2` and `Color`.

use crate::Ui;
use crate::animation::*;
use crate::common::time::MAX_ANIM_DT;
use crate::display::Display;
use crate::forest::element::Configure;
use crate::primitives::approx::EPS;
use crate::primitives::color::Color;
use crate::primitives::widget_id::WidgetId;
use crate::ui::frame::FrameStamp;
use crate::widgets::frame::Frame;
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(100, 100);
const SLOT: AnimSlot = AnimSlot("test");

/// Process-global counter handed to `AnimMapTyped::tick` for tests
/// that don't care about pass A/B semantics — every call gets a
/// fresh id, so the multi-pass guard never short-circuits unless a
/// test deliberately reuses an id. Tests that *do* exercise the
/// multi-pass guard pass an explicit `frame_id` literal instead.
fn next_frame() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed) + 1
}

fn wid(s: &'static str) -> WidgetId {
    WidgetId::from_hash(s)
}

#[derive(Debug)]
struct DurationMotionState<'a, T> {
    segment_start: &'a T,
    elapsed: f32,
}

fn duration_motion<T: Animatable>(row: &AnimRow<T>) -> DurationMotionState<'_, T> {
    let MotionRow::Duration {
        segment_start,
        elapsed,
    } = &row.motion
    else {
        panic!("expected duration motion state");
    };
    DurationMotionState {
        segment_start,
        elapsed: *elapsed,
    }
}

fn spring_velocity<T: Animatable>(row: &AnimRow<T>) -> &T {
    let MotionRow::Spring { velocity } = &row.motion else {
        panic!("expected spring motion state");
    };
    velocity
}

/// Common prelude for tests that drive an animated widget through
/// [`Ui::frame`]: spin up a `Ui`, pre-record the widget once so
/// its state row exists, return the `Ui`, the widget's id, and a
/// matching `Display`. Per-frame bodies still need to re-record the
/// widget (`Frame::new().id(WidgetId::from_hash(salt)).show(ui)`) so the persistent
/// state survives end-of-frame sweeps.
struct AnimUi {
    ui: Ui,
    id: WidgetId,
    display: Display,
}

fn setup_anim_ui(salt: &'static str) -> AnimUi {
    let mut ui = Ui::for_test();
    let id = wid(salt);
    ui.run_at(SURFACE, |ui| {
        Frame::new().id(WidgetId::from_hash(salt)).show(ui);
    });
    let display = Display::from_physical(SURFACE, 1.0);
    AnimUi { ui, id, display }
}

fn linear_100ms() -> AnimSpec {
    AnimSpec::duration(0.1, Easing::Linear)
}

#[test]
fn anim_spec_construction_validates_and_canonicalizes() {
    let instant_zero = AnimSpec::duration(0.0, Easing::Linear);
    let instant_negative_zero = AnimSpec::duration(-0.0, Easing::Linear);
    let instant_sub_eps = AnimSpec::duration(EPS * 0.5, Easing::Linear);
    assert!(instant_zero.is_instant());
    assert!(instant_negative_zero.is_instant());
    assert!(instant_sub_eps.is_instant());
    assert!(!AnimSpec::duration(EPS, Easing::Linear).is_instant());
    assert!(!AnimSpec::duration(60.0, Easing::Linear).is_instant());
    assert!(!AnimSpec::FAST.is_instant());
    assert!(!AnimSpec::SPRING.is_instant());

    for secs in [
        -1.0,
        60.0 + f32::EPSILON * 64.0,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ] {
        assert!(
            std::panic::catch_unwind(|| AnimSpec::duration(secs, Easing::Linear)).is_err(),
            "duration constructor accepted {secs:?}",
        );
    }

    for (stiffness, damping) in [
        (0.0, 1.0),
        (1.0, 0.0),
        (-1.0, 1.0),
        (1.0, -1.0),
        (f32::NAN, 1.0),
        (1.0, f32::INFINITY),
        (1.0, 1.0),
        (1.0, 100.0),
        (f32::MAX, 2.0),
    ] {
        assert!(
            std::panic::catch_unwind(|| AnimSpec::spring(stiffness, damping)).is_err(),
            "spring constructor accepted ({stiffness:?}, {damping:?})",
        );
    }

    assert!(!AnimSpec::spring(1.0, 2.0).is_instant());
    assert!(!AnimSpec::spring(1_000_000.0, 100.0).is_instant());
}

#[test]
fn anim_spec_serde_validates_and_roundtrips() {
    #[derive(::serde::Serialize, ::serde::Deserialize, PartialEq, Debug)]
    struct Holder {
        spec: AnimSpec,
    }
    let cases = [
        AnimSpec::FAST,
        AnimSpec::MEDIUM,
        AnimSpec::SPRING,
        AnimSpec::duration(0.1, Easing::Linear),
        AnimSpec::duration(0.2, Easing::InOutCubic),
        AnimSpec::duration(0.3, Easing::OutQuart),
        AnimSpec::duration(0.4, Easing::OutBack),
        AnimSpec::spring(100.0, 15.0),
        AnimSpec::spring(1_000_000.0, 100.0),
    ];
    for spec in cases {
        let h = Holder { spec };
        let s = toml::to_string(&h).expect("serialize");
        let back: Holder = toml::from_str(&s).expect("parse");
        assert_eq!(back, h, "roundtrip mismatch for {spec:?}\nTOML:\n{s}");
    }

    let canonical: Holder = toml::from_str(
        r#"
            [spec]
            kind = "duration"
            secs = 0.00005
            ease = "linear"
        "#,
    )
    .expect("sub-epsilon duration is a valid instant");
    assert!(canonical.spec.is_instant());
    assert!(
        toml::to_string(&canonical)
            .expect("serialize canonical duration")
            .contains("secs = 0.0"),
    );

    let invalid = [
        (
            "negative duration",
            r#"
                [spec]
                kind = "duration"
                secs = -1.0
                ease = "linear"
            "#,
            "animation duration must be finite and in 0.0..=60.0 seconds",
        ),
        (
            "non-finite duration",
            r#"
                [spec]
                kind = "duration"
                secs = nan
                ease = "linear"
            "#,
            "animation duration must be finite and in 0.0..=60.0 seconds",
        ),
        (
            "non-positive spring",
            r#"
                [spec]
                kind = "spring"
                stiffness = 170.0
                damping = 0.0
            "#,
            "spring parameters must be positive, finite, convergent, and within the integration limit",
        ),
        (
            "slow spring",
            r#"
                [spec]
                kind = "spring"
                stiffness = 1.0
                damping = 100.0
            "#,
            "spring parameters must be positive, finite, convergent, and within the integration limit",
        ),
        (
            "expensive spring",
            r#"
                [spec]
                kind = "spring"
                stiffness = 3.4028235e38
                damping = 2.0
            "#,
            "spring parameters must be positive, finite, convergent, and within the integration limit",
        ),
    ];
    for (label, input, expected) in invalid {
        let error = toml::from_str::<Holder>(input).expect_err(label);
        assert!(
            error.to_string().contains(expected),
            "{label}: unexpected serde error: {error}",
        );
    }
}

/// Through `Ui::animate`, a `Duration { secs = 0 }` spec behaves
/// identically to `None`: snaps to target, drops any in-flight row,
/// no repaint request. Switching from a real spec to instant-Duration
/// resets cleanly so a future real spec starts fresh.
#[test]
fn instant_duration_is_noop_and_drops_row() {
    let instant = Some(AnimSpec::duration(0.0, Easing::Linear));
    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("anim-instant");

    // Instant on a fresh slot: snaps, no row, no repaint.
    let repaint = ui
        .record(FrameStamp::new(display, Duration::from_millis(0)), |ui| {
            let v = ui.animate(id, SLOT, 1.0_f32, instant);
            assert_eq!(v, 1.0);
            Frame::new()
                .id(WidgetId::from_hash("anim-instant"))
                .show(ui);
        })
        .repaint_requested;
    assert!(!repaint);
    assert_eq!(ui.anim_row_count::<f32>(), 0);

    // Mid-flight on FAST: row gets allocated.
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(0)), |ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
        Frame::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(50)), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Frame::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    assert!(ui.anim_row_count::<f32>() > 0);

    // Switching to instant mid-flight: snap and drop.
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(60)), |ui| {
        let v = ui.animate(id, SLOT, 1.0_f32, instant);
        assert_eq!(v, 1.0);
        Frame::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    assert_eq!(
        ui.anim_row_count::<f32>(),
        0,
        "instant must drop the stale row inserted by FAST",
    );

    // Switching back to FAST with a new target: first-touch snaps.
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(70)), |ui| {
        let v = ui.animate(id, SLOT, 5.0_f32, Some(AnimSpec::FAST));
        assert_eq!(v, 5.0, "post-instant first-touch snaps to new target");
        Frame::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
}

/// Sub-perceptual drift between `target` and `current` must snap rather
/// than starting a full ease/spring cycle. Otherwise tiny float
/// quantization in the caller (rounded theme colors, sub-pixel rect
/// drift) would spuriously request repaints frame after frame for
/// changes the user can't see. The duration floor is `approx::EPS`
/// (1e-4), tighter than the spring floor (0.01), so a delta well under
/// 1e-4 snaps on *both* specs.
#[test]
fn target_below_snap_floor_snaps_without_animating() {
    let duration = AnimSpec::duration(1.0, Easing::Linear);
    let tiny = 1.0e-5; // below the duration floor (1e-4), the tighter one
    let cases: &[(&str, AnimSpec)] = &[("duration", duration), ("spring", AnimSpec::SPRING)];
    for (label, spec) in cases {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid("a");
        let _ = map.tick(id, SLOT, 0.0, *spec, 0.016, next_frame());
        let r = map.tick(id, SLOT, tiny, *spec, 0.016, next_frame());
        assert_eq!(
            r.current, tiny,
            "case {label}: snap-if-close must reach new target exactly",
        );
        assert!(
            r.settled,
            "case {label}: sub-eps drift must report settled (no repaint)",
        );
    }
}

/// The duration snap floor is far tighter than the spring floor: a
/// delta of 5e-4 sits inside the loose spring floor (0.01) but above
/// the tight duration floor (1e-4). So a spring snaps for that delta
/// while a duration runs its designed curve — a subtle colour
/// transition must not be silently swallowed just because the spring
/// path tolerates pixel-scale residue. Pins the deliberate split.
#[test]
fn duration_floor_is_tighter_than_spring_floor() {
    let delta = 5.0e-4_f32;

    let mut spring_map = AnimMapTyped::<f32>::default();
    let sid = wid("s");
    let _ = spring_map.tick(sid, SLOT, 0.0, AnimSpec::SPRING, 0.016, next_frame());
    let rs = spring_map.tick(sid, SLOT, delta, AnimSpec::SPRING, 0.016, next_frame());
    assert_eq!(rs.current, delta, "spring snaps within its loose floor");
    assert!(rs.settled, "spring reports settled after snap");

    let duration = AnimSpec::duration(1.0, Easing::Linear);
    let mut dur_map = AnimMapTyped::<f32>::default();
    let did = wid("d");
    let _ = dur_map.tick(did, SLOT, 0.0, duration, 0.016, next_frame());
    let rd = dur_map.tick(did, SLOT, delta, duration, 0.016, next_frame());
    // One linear step of 0.016/1.0 toward delta: 0.016 * 5e-4 = 8e-6.
    assert!(
        rd.current < delta && rd.current > 0.0,
        "duration animates toward target, not snap; got {}",
        rd.current,
    );
    assert!(!rd.settled, "duration mid-curve is not settled");
}

#[test]
fn first_touch_returns_target_and_settled() {
    for (label, spec) in [("duration", AnimSpec::FAST), ("spring", AnimSpec::SPRING)] {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid(label);
        let r = map.tick(id, SLOT, 1.0, spec, 0.016, next_frame());
        assert_eq!(r.current, 1.0, "{label}: first touch must snap");
        assert!(r.settled, "{label}: first touch must report settled");
        let row = &map.rows[&(id, SLOT)];
        match &row.motion {
            MotionRow::Duration {
                segment_start,
                elapsed,
            } => {
                assert_eq!((*segment_start, *elapsed), (1.0, 0.0));
                assert!(matches!(spec.motion, AnimMotion::Duration { .. }));
            }
            MotionRow::Spring { velocity } => {
                assert_eq!(*velocity, 0.0);
                assert!(matches!(spec.motion, AnimMotion::Spring { .. }));
            }
        }
    }
}

#[test]
fn duration_settles_in_finite_steps() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, 0.0, spec, 0.016, next_frame());
    let _ = map.tick(id, SLOT, 1.0, spec, 0.0, next_frame());
    let r = map.tick(id, SLOT, 1.0, spec, 0.05, next_frame());
    assert!(
        r.current > 0.4 && r.current < 0.6,
        "halfway should be ~0.5; got {}",
        r.current,
    );
    assert!(!r.settled, "halfway is not settled");
    let r = map.tick(id, SLOT, 1.0, spec, 0.05, next_frame());
    assert_eq!(r.current, 1.0, "must snap to target on settle");
    assert!(r.settled, "100ms total elapsed must settle");

    let mut boundary_map = AnimMapTyped::<f32>::default();
    let boundary_id = wid("maximum-duration");
    let boundary = AnimSpec::duration(60.0, Easing::Linear);
    let _ = boundary_map.tick(boundary_id, SLOT, 0.0, boundary, 0.0, next_frame());
    let mut settled = None;
    for step in 0..=600 {
        let result = boundary_map.tick(boundary_id, SLOT, 1.0, boundary, MAX_ANIM_DT, next_frame());
        assert!(result.current.is_finite());
        if result.settled {
            assert_eq!(result.current, 1.0);
            settled = Some(step);
            break;
        }
    }
    assert!(
        settled.is_some(),
        "maximum duration did not settle after 60.1 seconds",
    );
}

#[test]
fn retarget_mid_flight_starts_new_segment_from_current() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, 0.0, spec, 0.016, next_frame());
    let _ = map.tick(id, SLOT, 1.0, spec, 0.0, next_frame());
    let mid = map.tick(id, SLOT, 1.0, spec, 0.05, next_frame()).current;
    assert!(mid > 0.4 && mid < 0.6, "halfway to 1.0; got {mid}");

    let r = map.tick(id, SLOT, 2.0, spec, 0.0, next_frame());
    assert_eq!(r.current, mid, "retarget must preserve current");
    let r = map.tick(id, SLOT, 2.0, spec, 0.05, next_frame());
    let expected = (mid + 2.0) * 0.5;
    assert!(
        (r.current - expected).abs() < 0.01,
        "new segment should ease from mid to 2.0; got {} expected {}",
        r.current,
        expected,
    );
}

#[test]
fn dt_zero_does_not_advance_duration() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, 0.0, spec, 0.0, next_frame());
    let _ = map.tick(id, SLOT, 1.0, spec, 0.0, next_frame());
    let r = map.tick(id, SLOT, 1.0, spec, 0.0, next_frame());
    assert_eq!(r.current, 0.0, "dt=0 must not advance toward target");
    assert!(!r.settled, "still in flight");
}

#[test]
fn validated_springs_remain_finite_and_settle() {
    let cases = [
        ("minimum-decay", AnimSpec::spring(1.0, 2.0)),
        ("default", AnimSpec::SPRING),
        ("adaptive-step", AnimSpec::spring(1_000_000.0, 100.0)),
    ];
    let dts = [0.1, 1.0 / 60.0, 0.0042, 0.033];

    for (label, spec) in cases {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid(label);
        let _ = map.tick(id, SLOT, 400.0, spec, dts[0], next_frame());
        let mut settled_at = None;
        for i in 0..4_000 {
            let result = map.tick(id, SLOT, -100.0, spec, dts[i % dts.len()], next_frame());
            let row = &map.rows[&(id, SLOT)];
            let velocity = *spring_velocity(row);
            assert!(
                result.current.is_finite() && velocity.is_finite(),
                "{label} became non-finite at step {i}: {row:?}",
            );
            if result.settled {
                assert_eq!(result.current, -100.0, "{label} did not snap to target");
                assert_eq!(velocity, 0.0, "{label} retained settled velocity");
                settled_at = Some(i);
                break;
            }
        }
        assert!(
            settled_at.is_some(),
            "{label} did not settle under the deterministic frame sequence",
        );
    }
}

#[test]
fn built_in_spring_preserves_validated_substep() {
    let AnimMotion::Spring {
        stiffness,
        damping,
        substep_dt,
    } = AnimSpec::SPRING.motion
    else {
        panic!("built-in spring has the wrong motion kind");
    };
    assert!(spring::params_are_valid(stiffness, damping, substep_dt));
    assert_eq!(substep_dt, spring::stable_substep_dt(stiffness, damping));
}

#[test]
fn spring_parameters_change_trajectory() {
    let mut default_map = AnimMapTyped::<f32>::default();
    let mut custom_map = AnimMapTyped::<f32>::default();
    let id = wid("spring-parameters");
    let custom = AnimSpec::spring(100.0, 15.0);
    let _ = default_map.tick(id, SLOT, 0.0, AnimSpec::SPRING, 0.016, next_frame());
    let _ = custom_map.tick(id, SLOT, 0.0, custom, 0.016, next_frame());
    let default = default_map
        .tick(id, SLOT, 1.0, AnimSpec::SPRING, 0.016, next_frame())
        .current;
    let custom = custom_map
        .tick(id, SLOT, 1.0, custom, 0.016, next_frame())
        .current;
    assert_ne!(default, custom);
}

/// Worst-case wall-clock `dt` (= `Ui::MAX_DT` after a stalled frame
/// or a tab-switch redraw gap) must not blow up the integrator: a
/// single-step semi-implicit Euler at `dt = 0.1` with default spring
/// `(170, 26)` produces a `current` far past the target (negative for
/// the showcase animation widths, triggering the `Sizing::fixed`
/// invariant). Pin: stepping a 400→80 spring with `dt = 0.1` keeps
/// `current` within `[80, 400]`.
#[test]
fn spring_step_at_max_dt_stays_bounded() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let _ = map.tick(id, SLOT, 400.0, AnimSpec::SPRING, 0.016, next_frame());
    let r = map.tick(id, SLOT, 80.0, AnimSpec::SPRING, 0.1, next_frame());
    assert!(
        r.current >= 80.0 && r.current <= 400.0,
        "spring at dt=MAX_DT must stay between segment endpoints; got {}",
        r.current,
    );
}

/// A frame may run `build` twice on input frames (pass A
/// records, drains input, pass B re-records with the post-action
/// state). Both passes call `Ui::animate`, which dispatches to
/// `tick`. The multi-pass guard keys on `frame_id` so two ticks
/// sharing one — i.e. one wall-clock frame — only advance the
/// integrator once. Retargets in pass B must still take effect (the
/// next frame should ease toward the new target from pass A's
/// advanced position), but the second tick must not add another
/// `dt` of motion.
#[test]
fn second_tick_in_same_frame_does_not_double_advance() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let frame = 42;

    // Seed: row settled at 0.0. Different frame so we don't trip the
    // guard during setup.
    let _ = map.tick(id, SLOT, 0.0, AnimSpec::FAST, 0.016, frame - 1);

    // Pass A: target 1.0, advance one step.
    let pass_a = map.tick(id, SLOT, 1.0, AnimSpec::FAST, 0.016, frame);
    assert!(pass_a.current > 0.0 && pass_a.current < 1.0);
    let pass_a_current = pass_a.current;
    let pass_a_elapsed = duration_motion(&map.rows[&(id, SLOT)]).elapsed;

    // Pass B: same frame_id, same target. Must NOT advance further;
    // current and elapsed must match pass A exactly.
    let pass_b = map.tick(id, SLOT, 1.0, AnimSpec::FAST, 0.016, frame);
    assert_eq!(
        pass_b.current, pass_a_current,
        "pass B with same frame_id must not advance current",
    );
    assert_eq!(
        duration_motion(&map.rows[&(id, SLOT)]).elapsed,
        pass_a_elapsed,
        "pass B with same frame_id must not advance elapsed",
    );

    // Pass B with a *different* target (post-action retarget): the
    // segment resets so the next frame eases toward the new target,
    // but the current value is held at pass A's advanced position.
    let pass_b_retarget = map.tick(id, SLOT, 5.0, AnimSpec::FAST, 0.016, frame);
    assert_eq!(
        pass_b_retarget.current, pass_a_current,
        "retargeting in pass B updates segment but doesn't re-step",
    );
    assert_eq!(map.rows[&(id, SLOT)].target, 5.0);
    assert_eq!(
        *duration_motion(&map.rows[&(id, SLOT)]).segment_start,
        pass_a_current
    );

    // Next frame: integrator advances from the retargeted segment.
    let next = map.tick(id, SLOT, 5.0, AnimSpec::FAST, 0.016, frame + 1);
    assert!(
        next.current > pass_a_current,
        "next frame must advance toward 5.0 from pass A's current",
    );
}

/// Spring retarget into the path of motion keeps velocity (the
/// "fling-through" continuation); retarget *against* the velocity
/// zeroes it so the new segment can't swing far past the target.
/// Without the projection, a fast click-then-reverse can drag the
/// value well below zero / above any plausible bound; the
/// `Sizing::fixed` invariant in the showcase relied on this.
#[test]
fn spring_retarget_zeroes_opposing_velocity_only() {
    let mut map = AnimMapTyped::<f32>::default();

    // Aligned: moving toward 1.0, retarget further along the same
    // direction (2.0). Velocity should survive — that's the fling.
    let id_aligned = wid("aligned");
    let _ = map.tick(id_aligned, SLOT, 0.0, AnimSpec::SPRING, 0.016, next_frame());
    for _ in 0..3 {
        let _ = map.tick(id_aligned, SLOT, 1.0, AnimSpec::SPRING, 0.016, next_frame());
    }
    let v_before = *spring_velocity(&map.rows[&(id_aligned, SLOT)]);
    assert!(v_before > 0.0, "precondition: moving toward 1.0");
    let _ = map.tick(id_aligned, SLOT, 2.0, AnimSpec::SPRING, 0.0, next_frame());
    let v_after = *spring_velocity(&map.rows[&(id_aligned, SLOT)]);
    assert_eq!(v_after, v_before, "aligned retarget must preserve velocity");

    // Opposed: moving toward 1.0, retarget backward to -1.0. Velocity
    // points away from the new target — zero it.
    let id_opposed = wid("opposed");
    let _ = map.tick(id_opposed, SLOT, 0.0, AnimSpec::SPRING, 0.016, next_frame());
    for _ in 0..3 {
        let _ = map.tick(id_opposed, SLOT, 1.0, AnimSpec::SPRING, 0.016, next_frame());
    }
    assert!(
        *spring_velocity(&map.rows[&(id_opposed, SLOT)]) > 0.0,
        "precondition: moving toward 1.0"
    );
    let _ = map.tick(id_opposed, SLOT, -1.0, AnimSpec::SPRING, 0.0, next_frame());
    assert_eq!(
        *spring_velocity(&map.rows[&(id_opposed, SLOT)]),
        0.0,
        "opposing retarget must zero velocity to kill reversal overshoot",
    );
}

#[test]
fn vec2_duration_lerps_componentwise() {
    let mut map = AnimMapTyped::<Vec2>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, Vec2::ZERO, spec, 0.0, next_frame());
    let _ = map.tick(id, SLOT, Vec2::new(10.0, 20.0), spec, 0.0, next_frame());
    let r = map.tick(id, SLOT, Vec2::new(10.0, 20.0), spec, 0.05, next_frame());
    assert!(
        (r.current.x - 5.0).abs() < 0.01 && (r.current.y - 10.0).abs() < 0.01,
        "halfway should be (5, 10); got {:?}",
        r.current,
    );
}

#[test]
fn color_spring_converges_to_target() {
    let mut map = AnimMapTyped::<Color>::default();
    let id = wid("a");
    let start = Color::rgb(0.0, 0.0, 0.0);
    let target = Color::rgb(1.0, 0.5, 0.25);
    let _ = map.tick(id, SLOT, start, AnimSpec::SPRING, 0.016, next_frame());
    let mut last = start;
    let mut settled_at = None;
    for i in 0..600 {
        let r = map.tick(id, SLOT, target, AnimSpec::SPRING, 0.016, next_frame());
        last = r.current;
        if r.settled {
            settled_at = Some(i);
            break;
        }
    }
    assert!(
        settled_at.is_some(),
        "color spring must settle; last = {last:?}",
    );
    assert!(
        (last.r - target.r).abs() < 0.01
            && (last.g - target.g).abs() < 0.01
            && (last.b - target.b).abs() < 0.01,
        "settled color must match target; got {last:?} expected {target:?}",
    );
}

#[test]
fn solid_brush_spring_matches_color_trajectory() {
    use crate::primitives::brush::Brush;

    let mut color_map = AnimMapTyped::<Color>::default();
    let mut brush_map = AnimMapTyped::<Brush>::default();
    let color_id = wid("solid-color-trajectory");
    let brush_id = wid("solid-brush-trajectory");
    let start = Color::rgba(0.1, 0.2, 0.3, 0.4);
    let target = Color::rgba(0.9, 0.7, 0.5, 0.8);
    let _ = color_map.tick(color_id, SLOT, start, AnimSpec::SPRING, 0.0, next_frame());
    let _ = brush_map.tick(
        brush_id,
        SLOT,
        Brush::Solid(start),
        AnimSpec::SPRING,
        0.0,
        next_frame(),
    );

    let mut settled = false;
    for _ in 0..600 {
        let color = color_map.tick(
            color_id,
            SLOT,
            target,
            AnimSpec::SPRING,
            0.016,
            next_frame(),
        );
        let brush = brush_map.tick(
            brush_id,
            SLOT,
            Brush::Solid(target),
            AnimSpec::SPRING,
            0.016,
            next_frame(),
        );
        assert_eq!(brush.current.as_solid(), Some(color.current));
        assert_eq!(brush.settled, color.settled);
        settled = brush.settled;
        if settled {
            break;
        }
    }
    assert!(settled, "solid brush and color springs must both settle");
}

/// End-to-end through `Ui::animate` + `FrameOutput::repaint_requested`:
/// first-touch settled → no repaint; retarget in-flight → repaint;
/// repeated frames eventually settle and stop requesting repaint.
#[test]
fn animate_drives_repaint_until_settle() {
    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("anim-test");

    let repaint = ui
        .record(FrameStamp::new(display, Duration::ZERO), |ui| {
            let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
            Frame::new().id(WidgetId::from_hash("anim-test")).show(ui);
        })
        .repaint_requested;
    assert!(
        !repaint,
        "first-touch settled animation must not request repaint",
    );

    let repaint = ui
        .record(FrameStamp::new(display, Duration::from_millis(16)), |ui| {
            let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
            Frame::new().id(WidgetId::from_hash("anim-test")).show(ui);
        })
        .repaint_requested;
    assert!(repaint, "in-flight animation must request repaint");

    let mut now = Duration::from_millis(16);
    let mut settled_at = None;
    for i in 0..100 {
        now += Duration::from_millis(16);
        let repaint = ui
            .record(FrameStamp::new(display, now), |ui| {
                let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
                Frame::new().id(WidgetId::from_hash("anim-test")).show(ui);
            })
            .repaint_requested;
        if !repaint {
            settled_at = Some(i);
            break;
        }
    }
    assert!(
        settled_at.is_some(),
        "animation must settle and stop requesting repaints",
    );
}

/// Pin the fixed-step accumulator on `Ui`: a `Ui::frame` loop driven
/// at NoVsync-style sub-millisecond `dt` must still settle a spring
/// retarget. Pre-fix, `cur += vel·dt` would fall below the f32 ULP at
/// pixel-scale positions, the integrator would stall short of
/// `POS_EPS`, and `repaint_requested` would stay armed forever.
#[test]
fn spring_settles_under_sub_millisecond_dt_via_fixed_step_accumulator() {
    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("anim-novsync");

    // First touch at target=80 → snap, no repaint.
    let mut now = Duration::ZERO;
    let _ = ui.record(FrameStamp::new(display, now), |ui| {
        let _ = ui.animate(id, SLOT, 80.0_f32, Some(AnimSpec::SPRING));
        Frame::new()
            .id(WidgetId::from_hash("anim-novsync"))
            .show(ui);
    });

    // Retarget to 400 over a tight loop with 10 µs per frame (NoVsync).
    let mut settled_at = None;
    for i in 0..200_000 {
        now += Duration::from_micros(10);
        let repaint = ui
            .record(FrameStamp::new(display, now), |ui| {
                let _ = ui.animate(id, SLOT, 400.0_f32, Some(AnimSpec::SPRING));
                Frame::new()
                    .id(WidgetId::from_hash("anim-novsync"))
                    .show(ui);
            })
            .repaint_requested;
        if !repaint {
            settled_at = Some(i);
            break;
        }
    }
    assert!(
        settled_at.is_some(),
        "spring must settle under sub-millisecond dt",
    );
}

#[test]
fn removed_widget_evicts_all_slots_across_typed_maps() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let other = wid("b");
    let _ =
        map.typed_mut::<f32>()
            .tick(id, AnimSlot("a"), 1.0, AnimSpec::FAST, 0.016, next_frame());
    let _ =
        map.typed_mut::<f32>()
            .tick(id, AnimSlot("b"), 2.0, AnimSpec::FAST, 0.016, next_frame());
    let _ = map.typed_mut::<Vec2>().tick(
        id,
        AnimSlot("a"),
        Vec2::ONE,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    let _ = map.typed_mut::<Color>().tick(
        id,
        AnimSlot("a"),
        Color::rgb(1.0, 0.0, 0.0),
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    let _ = map.typed_mut::<f32>().tick(
        other,
        AnimSlot("a"),
        9.0,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    // No `Ui` here — reach into typed maps via `try_typed_mut`
    // (immutable peek goes through the same `as_any_mut` downcast
    // path; we just read `.rows.len()`).
    let f = |m: &mut AnimMap| m.try_typed_mut::<f32>().map_or(0, |t| t.rows.len());
    let v = |m: &mut AnimMap| m.try_typed_mut::<Vec2>().map_or(0, |t| t.rows.len());
    let c = |m: &mut AnimMap| m.try_typed_mut::<Color>().map_or(0, |t| t.rows.len());
    assert_eq!(f(&mut map), 3);
    assert_eq!(v(&mut map), 1);
    assert_eq!(c(&mut map), 1);

    map.sweep_removed(&FxHashSet::from_iter([id]));
    assert_eq!(
        f(&mut map),
        1,
        "scalar slots for `id` must drop, `other` survives",
    );
    assert_eq!(v(&mut map), 0, "vec2 slots for `id` must drop");
    assert_eq!(c(&mut map), 0, "color slots for `id` must drop");
}

/// `post_record` also evicts slots that were *not* poked this frame
/// even when the widget id itself stuck around — without this a
/// `(WidgetId, AnimSlot)` whose owner stopped calling
/// `Ui::animate` would linger forever, since the only other drop
/// trigger is full widget removal.
#[test]
fn post_record_evicts_untouched_slots() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let empty = FxHashSet::default();

    // Touch two slots, then run `post_record` to commit a "frame":
    // both rows survive, both `touched` flags clear.
    let _ =
        map.typed_mut::<f32>()
            .tick(id, AnimSlot("a"), 1.0, AnimSpec::FAST, 0.016, next_frame());
    let _ =
        map.typed_mut::<f32>()
            .tick(id, AnimSlot("b"), 2.0, AnimSpec::FAST, 0.016, next_frame());
    map.sweep_removed(&empty);
    let count = |m: &mut AnimMap| m.try_typed_mut::<f32>().map_or(0, |t| t.rows.len());
    assert_eq!(
        count(&mut map),
        2,
        "both slots must survive the first sweep"
    );

    // Next frame: only poke slot 0. Slot 1 was never re-touched
    // after `post_record` cleared its flag, so it should drop.
    let _ =
        map.typed_mut::<f32>()
            .tick(id, AnimSlot("a"), 1.0, AnimSpec::FAST, 0.016, next_frame());
    map.sweep_removed(&empty);
    assert_eq!(
        count(&mut map),
        1,
        "abandoned slot must drop while the still-poked slot survives",
    );

    // Re-poke slot 1 — first-touch path snaps to target. Confirms
    // dropped rows behave like any other never-seen `(id, slot)`.
    let r =
        map.typed_mut::<f32>()
            .tick(id, AnimSlot("b"), 99.0, AnimSpec::FAST, 0.016, next_frame());
    assert_eq!(r.current, 99.0);
    assert!(r.settled, "re-touch after eviction is a fresh first-touch");
}

/// `Ui::animate(..., None)` must: return `target` unchanged, never
/// allocate a row, never request a repaint. `None` is the API-level
/// signal "this caller didn't ask for motion."
#[test]
fn animate_with_none_spec_snaps_and_skips_repaint() {
    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("anim-none");
    let repaint = ui
        .record(FrameStamp::new(display, Duration::from_millis(16)), |ui| {
            let v1 = ui.animate(id, SLOT, 7.0_f32, None);
            let v2 = ui.animate(id, SLOT, 9.0_f32, None);
            assert_eq!(v1, 7.0);
            assert_eq!(v2, 9.0);
            Frame::new().id(WidgetId::from_hash("anim-none")).show(ui);
        })
        .repaint_requested;
    assert!(!repaint, "None spec must never request a repaint");
    assert!(
        ui.anim_row_count::<f32>() == 0,
        "None spec must not allocate a row",
    );
}

/// Switching from `Some(spec)` to `None` mid-flight must drop the
/// stale row so a future `Some(spec)` retarget starts fresh from the
/// new target rather than carrying in-flight `current` forward.
#[test]
fn animate_some_then_none_drops_stale_row() {
    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("anim-toggle");
    // Frame A: animate to 1.0 with FAST (in flight).
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(0)), |ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
        Frame::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(50)), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Frame::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    assert!(
        ui.anim_row_count::<f32>() > 0,
        "Some(FAST) must allocate a row mid-flight",
    );

    // Frame B: switch to None — the stale row should drop.
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(60)), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, None);
        Frame::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    assert!(
        ui.anim_row_count::<f32>() == 0,
        "None spec must drop the stale row inserted by a prior Some()",
    );
}

/// `WidgetLook::animate` resolves the look's optional components to
/// flat values and returns an `AnimatedLook` with the right defaults.
/// Walks both branches: with `spec = None` (snap, no rows) and with a
/// real spec (rows allocated for non-trivial components).
#[test]
fn widget_look_animate_resolves_components_and_falls_back() {
    use crate::primitives::background::Background;
    use crate::primitives::corners::Corners;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;
    use crate::widgets::theme::text_style::TextStyle;
    use crate::widgets::theme::widget_look::{AnimatedLook, WidgetLook};
    use std::cell::Cell;

    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("look-test");

    let bg = Background {
        fill: Color::hex(0x336699).into(),
        stroke: Stroke::solid(Color::hex(0xffffff), 2.0),
        corners: Corners::all(4.0),
        shadow: Shadow::NONE,
    };
    let look = WidgetLook {
        background: Some(bg.clone()),
        text: None, // → falls back to TextStyle default
    };
    let fallback = TextStyle::default();

    // None spec: snaps to target, no rows allocated. Use Cell to
    // capture out of the FnMut closure.
    let captured: Cell<Option<AnimatedLook>> = Cell::new(None);
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(16)), |ui| {
        captured.set(Some(look.clone().animate(ui, id, &fallback, None)));
        Frame::new().id(WidgetId::from_hash("look-test")).show(ui);
    });
    let snap = captured.take().expect("animate ran");
    assert_eq!(snap.background.fill, bg.fill, "None: fill snaps to target");
    assert_eq!(
        snap.background.stroke.width, 2.0,
        "None: stroke width snaps"
    );
    assert_eq!(snap.background.stroke.color, bg.stroke.color);
    assert_eq!(snap.background.corners, bg.corners);
    assert_eq!(
        snap.text.color, fallback.color,
        "None: text falls back to fallback_text",
    );
    assert_eq!(snap.text.font_size_px, fallback.font_size_px);
    assert_eq!(snap.text.line_height_mult, fallback.line_height_mult);
    assert_eq!(
        ui.anim_row_count::<AnimatedLook>(),
        0,
        "None spec: WidgetLook::animate must allocate no AnimatedLook row",
    );

    // Some(FAST) spec, retargeting to a different fill: a row gets
    // allocated for the in-flight Background animation. Text didn't
    // change, so the snap-if-close fast path leaves TextStyle row
    // unallocated.
    let look2 = WidgetLook {
        background: Some(Background {
            fill: Color::hex(0xff0000).into(),
            ..bg.clone()
        }),
        text: None,
    };
    let _ = ui.record(FrameStamp::new(display, Duration::from_millis(32)), |ui| {
        let _ = look2
            .clone()
            .animate(ui, id, &fallback, Some(AnimSpec::FAST));
        Frame::new().id(WidgetId::from_hash("look-test")).show(ui);
    });
    assert!(
        ui.anim_row_count::<AnimatedLook>() > 0,
        "Some(FAST) on changed fill must allocate an AnimatedLook row",
    );
}

/// Pin: `#[animate(snap)]` fields update on retarget mid-spring, not
/// on settle. `Background.radius` is snap; without the
/// `lerp(_, target, 0.0)` carry in spring `step`, the new radius
/// would only land when the spring snaps to target.
#[test]
fn spring_snap_fields_carry_target_immediately() {
    use crate::primitives::background::Background;
    use crate::primitives::corners::Corners;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;

    let mut map = AnimMapTyped::<Background>::default();
    let id = wid("snap-carry");
    let start = Background {
        fill: Color::rgb(0.0, 0.0, 0.0).into(),
        stroke: Stroke::ZERO,
        corners: Corners::all(2.0),
        shadow: Shadow::NONE,
    };
    // First touch: snaps current = start, returns settled. No motion
    // started yet.
    let _ = map.tick(id, SLOT, start, AnimSpec::SPRING, 0.016, next_frame());

    // Retarget to a new fill (animated) and a new radius (snap).
    let target = Background {
        fill: Color::rgb(1.0, 0.0, 0.0).into(),
        stroke: Stroke::ZERO,
        corners: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let r = map.tick(
        id,
        SLOT,
        target.clone(),
        AnimSpec::SPRING,
        0.016,
        next_frame(),
    );
    assert!(
        !r.settled,
        "spring with a real fill diff must remain in flight after one step",
    );
    assert_eq!(
        r.current.corners, target.corners,
        "snap field must carry target value on the first stepped frame, not lag until settle",
    );
    assert!(
        r.current.fill.as_solid().unwrap().r < target.fill.as_solid().unwrap().r - 0.05,
        "animated fill should still be mid-flight; got {:?}",
        r.current.fill,
    );
}

#[test]
fn gradient_snap_clears_only_its_background_velocity() {
    use crate::primitives::background::Background;
    use crate::primitives::brush::{Brush, LinearGradient};
    use crate::primitives::corners::Corners;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;

    let mut map = AnimMapTyped::<Background>::default();
    let id = wid("gradient-background-velocity");
    let start = Background {
        fill: Brush::Solid(Color::BLACK),
        stroke: Stroke::solid(Color::BLACK, 0.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let moving = Background {
        fill: Brush::Solid(Color::WHITE),
        stroke: Stroke::solid(Color::BLACK, 10.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let _ = map.tick(id, SLOT, start, AnimSpec::SPRING, 0.0, next_frame());
    for _ in 0..3 {
        let _ = map.tick(
            id,
            SLOT,
            moving.clone(),
            AnimSpec::SPRING,
            0.016,
            next_frame(),
        );
    }
    let stroke_velocity = spring_velocity(&map.rows[&(id, SLOT)]).stroke.width;
    assert!(
        stroke_velocity > 0.0,
        "test setup must carry positive stroke velocity",
    );

    let gradient = Brush::Linear(LinearGradient::two_stop(0.0, Color::BLACK, Color::WHITE));
    let target = Background {
        fill: gradient.clone(),
        stroke: Stroke::solid(Color::BLACK, 20.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let result = map.tick(id, SLOT, target, AnimSpec::SPRING, 0.0, next_frame());
    let row = &map.rows[&(id, SLOT)];
    let velocity = spring_velocity(row);
    assert_eq!(result.current.fill, gradient);
    assert_eq!(velocity.fill, Brush::TRANSPARENT);
    assert_eq!(velocity.stroke.width, stroke_velocity);
    assert!(
        !result.settled,
        "the independently animated stroke still has real displacement",
    );
}

#[test]
fn gradient_snap_inside_look_repaints_only_until_numeric_fields_settle() {
    use crate::primitives::background::Background;
    use crate::primitives::brush::{Brush, RadialGradient};
    use crate::widgets::theme::text_style::TextStyle;
    use crate::widgets::theme::widget_look::AnimatedLook;

    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("gradient-look-settle");
    let start = AnimatedLook {
        background: Background::fill(Color::BLACK),
        text: TextStyle::default().with_color(Color::BLACK),
    };
    let gradient = Brush::Radial(RadialGradient::two_stop_centered(
        Color::BLACK,
        Color::WHITE,
    ));
    let target = AnimatedLook {
        background: Background::fill(gradient.clone()),
        text: TextStyle::default().with_color(Color::WHITE),
    };

    let first = ui.record(FrameStamp::new(display, Duration::ZERO), |ui| {
        let current = ui.animate(id, SLOT, start.clone(), Some(AnimSpec::SPRING));
        assert_eq!(current, start);
        Frame::new()
            .id(WidgetId::from_hash("gradient-look-settle"))
            .show(ui);
    });
    assert!(!first.repaint_requested);

    let mut now = Duration::from_millis(16);
    let retarget = ui.record(FrameStamp::new(display, now), |ui| {
        let current = ui.animate(id, SLOT, target.clone(), Some(AnimSpec::SPRING));
        assert_eq!(current.background.fill, gradient);
        assert_ne!(current.text.color, target.text.color);
        Frame::new()
            .id(WidgetId::from_hash("gradient-look-settle"))
            .show(ui);
    });
    assert!(retarget.repaint_requested);

    let mut settled_at = None;
    for frame in 0..600 {
        now += Duration::from_millis(16);
        let mut current = target.clone();
        let output = ui.record(FrameStamp::new(display, now), |ui| {
            current = ui.animate(id, SLOT, target.clone(), Some(AnimSpec::SPRING));
            assert_eq!(current.background.fill, gradient);
            Frame::new()
                .id(WidgetId::from_hash("gradient-look-settle"))
                .show(ui);
        });
        if !output.repaint_requested {
            assert_eq!(current, target);
            settled_at = Some(frame);
            break;
        }
    }
    assert!(settled_at.is_some(), "the look's color spring must settle");

    now += Duration::from_millis(16);
    let after_settle = ui.record(FrameStamp::new(display, now), |ui| {
        let current = ui.animate(id, SLOT, target.clone(), Some(AnimSpec::SPRING));
        assert_eq!(current, target);
        Frame::new()
            .id(WidgetId::from_hash("gradient-look-settle"))
            .show(ui);
    });
    assert!(
        !after_settle.repaint_requested,
        "a settled look must not request a surplus repaint",
    );
}

#[test]
fn spring_to_duration_same_target_restarts_from_current() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("spec-switch");
    let _ = map.tick(id, SLOT, 0.0_f32, AnimSpec::SPRING, 0.016, next_frame());
    for _ in 0..5 {
        let _ = map.tick(id, SLOT, 1.0_f32, AnimSpec::SPRING, 0.016, next_frame());
    }
    let row = map.rows.get(&(id, SLOT)).expect("row exists mid-spring");
    let segment_start = row.current;
    let velocity = *spring_velocity(row);
    assert!(
        velocity.abs() > 0.01,
        "test setup: spring should have built up velocity by now; got {}",
        velocity,
    );

    let dur = AnimSpec::duration(0.1, Easing::Linear);
    let dt = 0.02;
    let result = map.tick(id, SLOT, 1.0_f32, dur, dt, next_frame());
    let row = map.rows.get(&(id, SLOT)).expect("row exists post-switch");
    let progress = dt / 0.1;
    let expected = segment_start + (1.0 - segment_start) * progress;
    let motion = duration_motion(row);
    assert_eq!(*motion.segment_start, segment_start);
    assert_eq!(motion.elapsed, dt);
    assert_eq!(result.current, expected);
}

#[test]
fn duration_to_spring_to_duration_same_target_restarts_each_mode() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("round-trip-spec-switch");
    let duration = AnimSpec::duration(1.0, Easing::Linear);
    let _ = map.tick(id, SLOT, 0.0, duration, 0.0, next_frame());
    let duration_result = map.tick(id, SLOT, 1.0, duration, 0.4, next_frame());
    assert_eq!(duration_result.current, 0.4);

    let spring_result = map.tick(id, SLOT, 1.0, AnimSpec::SPRING, 0.016, next_frame());
    let spring_row = map.rows.get(&(id, SLOT)).expect("row exists mid-spring");
    assert!(*spring_velocity(spring_row) > 0.0);

    let segment_start = spring_result.current;
    let dt = 0.25;
    let duration_result = map.tick(id, SLOT, 1.0, duration, dt, next_frame());
    let duration_row = map
        .rows
        .get(&(id, SLOT))
        .expect("row exists after duration restart");
    let expected = segment_start + (1.0 - segment_start) * dt;
    let motion = duration_motion(duration_row);
    assert_eq!(*motion.segment_start, segment_start);
    assert_eq!(motion.elapsed, dt);
    assert_eq!(duration_result.current, expected);
}
