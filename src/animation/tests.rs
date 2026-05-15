//! Pin tests for the animation primitive (generic over `Animatable`).
//! Covers first-touch, duration interpolation, retarget mid-flight,
//! spring convergence, settle clears repaint, removed-widget eviction,
//! plus typed-slot dispatch via `Vec2` and `Color`.

use super::*;
use crate::Ui;
use crate::forest::element::Configure;
use crate::layout::types::display::Display;
use crate::primitives::color::Color;
use crate::primitives::widget_id::WidgetId;
use crate::support::testing::run_at;
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

/// Common prelude for tests that drive an animated widget through
/// [`Ui::run_frame`]: spin up a `Ui`, pre-record the widget once so
/// its state row exists, return the `Ui`, the widget's id, and a
/// matching `Display`. Per-frame bodies still need to re-record the
/// widget (`Frame::new().id_salt(salt).show(ui)`) so the persistent
/// state survives end-of-frame sweeps.
struct AnimUi {
    ui: Ui,
    id: WidgetId,
    display: Display,
}

fn setup_anim_ui(salt: &'static str) -> AnimUi {
    let mut ui = Ui::default();
    let id = wid(salt);
    run_at(&mut ui, SURFACE, |ui| {
        Frame::new().id_salt(salt).show(ui);
    });
    let display = Display::from_physical(SURFACE, 1.0);
    AnimUi { ui, id, display }
}

fn linear_100ms() -> AnimSpec {
    AnimSpec::Duration {
        secs: 0.1,
        ease: Easing::Linear,
    }
}

/// `AnimSpec::is_instant()` predicate: classifies degenerate
/// `Duration { secs ≈ 0 }` as instant; springs are never instant.
/// `Ui::animate` uses this to merge instant Duration into the snap
/// path — same shape as passing `None`.
#[test]
fn is_instant_predicate() {
    let instant_zero = AnimSpec::Duration {
        secs: 0.0,
        ease: Easing::Linear,
    };
    let instant_neg = AnimSpec::Duration {
        secs: -1.0,
        ease: Easing::Linear,
    };
    assert!(instant_zero.is_instant());
    assert!(instant_neg.is_instant());
    assert!(!AnimSpec::FAST.is_instant());
    assert!(!AnimSpec::SPRING.is_instant());
}

/// `AnimSpec` (with `Easing` payload on the `Duration` variant)
/// round-trips through TOML. `ButtonTheme.anim` /
/// `TextEditTheme.anim` are public theme-file fields, so the
/// internally-tagged-on-`kind` representation has to stay stable
/// across refactors. Without this test a renamed variant or a
/// dropped `#[serde(rename_all = "snake_case")]` would compile and
/// ship silently — no production path actually deserializes a theme
/// with `anim` set today, so the derives have no other consumer.
///
/// Wrapped in a holder struct because TOML's top-level value must be
/// a table; `Easing` (string-shaped variants) wouldn't serialize on
/// its own at the root.
#[test]
fn anim_spec_serde_roundtrip() {
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Holder {
        spec: AnimSpec,
    }
    let cases = [
        AnimSpec::FAST,
        AnimSpec::MEDIUM,
        AnimSpec::SPRING,
        AnimSpec::Duration {
            secs: 0.1,
            ease: Easing::Linear,
        },
        AnimSpec::Duration {
            secs: 0.2,
            ease: Easing::InOutCubic,
        },
        AnimSpec::Duration {
            secs: 0.3,
            ease: Easing::OutQuart,
        },
        AnimSpec::Duration {
            secs: 0.4,
            ease: Easing::OutBack,
        },
        AnimSpec::Spring {
            stiffness: 100.0,
            damping: 15.0,
        },
    ];
    for spec in cases {
        let h = Holder { spec };
        let s = toml::to_string(&h).expect("serialize");
        let back: Holder = toml::from_str(&s).expect("parse");
        assert_eq!(back, h, "roundtrip mismatch for {spec:?}\nTOML:\n{s}");
    }
}

/// Through `Ui::animate`, a `Duration { secs = 0 }` spec behaves
/// identically to `None`: snaps to target, drops any in-flight row,
/// no repaint request. Switching from a real spec to instant-Duration
/// resets cleanly so a future real spec starts fresh.
#[test]
fn instant_duration_is_noop_and_drops_row() {
    let instant = Some(AnimSpec::Duration {
        secs: 0.0,
        ease: Easing::Linear,
    });
    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("anim-instant");

    // Instant on a fresh slot: snaps, no row, no repaint.
    let repaint = ui
        .frame(display, Duration::from_millis(0), &mut (), |ui| {
            let v = ui.animate(id, SLOT, 1.0_f32, instant);
            assert_eq!(v, 1.0);
            Frame::new().id_salt("anim-instant").show(ui);
        })
        .repaint_requested();
    assert!(!repaint);
    assert_eq!(crate::support::internals::anim_row_count::<f32>(&mut ui), 0);

    // Mid-flight on FAST: row gets allocated.
    let _ = ui.frame(display, Duration::from_millis(0), &mut (), |ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
        Frame::new().id_salt("anim-instant").show(ui);
    });
    let _ = ui.frame(display, Duration::from_millis(50), &mut (), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Frame::new().id_salt("anim-instant").show(ui);
    });
    assert!(crate::support::internals::anim_row_count::<f32>(&mut ui) > 0);

    // Switching to instant mid-flight: snap and drop.
    let _ = ui.frame(display, Duration::from_millis(60), &mut (), |ui| {
        let v = ui.animate(id, SLOT, 1.0_f32, instant);
        assert_eq!(v, 1.0);
        Frame::new().id_salt("anim-instant").show(ui);
    });
    assert_eq!(
        crate::support::internals::anim_row_count::<f32>(&mut ui),
        0,
        "instant must drop the stale row inserted by FAST",
    );

    // Switching back to FAST with a new target: first-touch snaps.
    let _ = ui.frame(display, Duration::from_millis(70), &mut (), |ui| {
        let v = ui.animate(id, SLOT, 5.0_f32, Some(AnimSpec::FAST));
        assert_eq!(v, 5.0, "post-instant first-touch snaps to new target");
        Frame::new().id_salt("anim-instant").show(ui);
    });
}

/// Sub-epsilon drift between `target` and `current` must snap rather
/// than starting a full ease/spring cycle. Otherwise tiny float
/// quantization in the caller (rounded theme colors, sub-pixel rect
/// drift) would spuriously request repaints frame after frame for
/// changes the user can't see.
#[test]
fn target_within_settle_eps_snaps_without_animating() {
    let duration = AnimSpec::Duration {
        secs: 1.0,
        ease: Easing::Linear,
    };
    let cases: &[(&str, AnimSpec)] = &[("duration", duration), ("spring", AnimSpec::SPRING)];
    for (label, spec) in cases {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid("a");
        let _ = map.tick(id, SLOT, 0.0, *spec, 0.016, next_frame());
        let r = map.tick(id, SLOT, 0.0005, *spec, 0.016, next_frame());
        assert_eq!(
            r.current, 0.0005,
            "case {label}: snap-if-close must reach new target exactly",
        );
        assert!(
            r.settled,
            "case {label}: sub-eps drift must report settled (no repaint)",
        );
    }
}

#[test]
fn first_touch_returns_target_and_settled() {
    let mut map = AnimMapTyped::<f32>::default();
    let r = map.tick(wid("a"), SLOT, 1.0, AnimSpec::FAST, 0.016, next_frame());
    assert_eq!(r.current, 1.0, "first touch must snap to target");
    assert!(r.settled, "first touch must report settled (no anim)");
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
fn spring_with_initial_displacement_converges_within_settle_eps() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let _ = map.tick(id, SLOT, 0.0, AnimSpec::SPRING, 0.016, next_frame());
    let mut last = f32::NAN;
    let mut settled_at = None;
    for i in 0..600 {
        let r = map.tick(id, SLOT, 1.0, AnimSpec::SPRING, 0.016, next_frame());
        last = r.current;
        if r.settled {
            settled_at = Some(i);
            break;
        }
    }
    assert!(
        settled_at.is_some(),
        "spring must settle within 600 frames (10s @ 60Hz); last = {last}",
    );
    assert!(
        (last - 1.0).abs() < 0.01,
        "settled value must equal target within eps; got {last}",
    );
}

/// Worst-case wall-clock `dt` (= `Ui::MAX_DT` after a stalled frame
/// or a tab-switch redraw gap) must not blow up the integrator: a
/// single-step semi-implicit Euler at `dt = 0.1` with default spring
/// `(170, 26)` produces a `current` far past the target (negative for
/// the showcase animation widths, triggering the `Sizing::Fixed`
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

/// `run_frame` may run `build` twice on input frames (pass A
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
    let pass_a_elapsed = map.rows[&(id, SLOT)].elapsed;

    // Pass B: same frame_id, same target. Must NOT advance further;
    // current and elapsed must match pass A exactly.
    let pass_b = map.tick(id, SLOT, 1.0, AnimSpec::FAST, 0.016, frame);
    assert_eq!(
        pass_b.current, pass_a_current,
        "pass B with same frame_id must not advance current",
    );
    assert_eq!(
        map.rows[&(id, SLOT)].elapsed,
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
    assert_eq!(map.rows[&(id, SLOT)].segment_start, pass_a_current);

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
/// `Sizing::Fixed` invariant in the showcase relied on this.
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
    let v_before = map.rows[&(id_aligned, SLOT)].velocity;
    assert!(v_before > 0.0, "precondition: moving toward 1.0");
    let _ = map.tick(id_aligned, SLOT, 2.0, AnimSpec::SPRING, 0.0, next_frame());
    let v_after = map.rows[&(id_aligned, SLOT)].velocity;
    assert_eq!(v_after, v_before, "aligned retarget must preserve velocity");

    // Opposed: moving toward 1.0, retarget backward to -1.0. Velocity
    // points away from the new target — zero it.
    let id_opposed = wid("opposed");
    let _ = map.tick(id_opposed, SLOT, 0.0, AnimSpec::SPRING, 0.016, next_frame());
    for _ in 0..3 {
        let _ = map.tick(id_opposed, SLOT, 1.0, AnimSpec::SPRING, 0.016, next_frame());
    }
    assert!(
        map.rows[&(id_opposed, SLOT)].velocity > 0.0,
        "precondition: moving toward 1.0"
    );
    let _ = map.tick(id_opposed, SLOT, -1.0, AnimSpec::SPRING, 0.0, next_frame());
    assert_eq!(
        map.rows[&(id_opposed, SLOT)].velocity,
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
        .frame(display, Duration::ZERO, &mut (), |ui| {
            let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
            Frame::new().id_salt("anim-test").show(ui);
        })
        .repaint_requested();
    assert!(
        !repaint,
        "first-touch settled animation must not request repaint",
    );

    let repaint = ui
        .frame(display, Duration::from_millis(16), &mut (), |ui| {
            let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
            Frame::new().id_salt("anim-test").show(ui);
        })
        .repaint_requested();
    assert!(repaint, "in-flight animation must request repaint");

    let mut now = Duration::from_millis(16);
    let mut settled_at = None;
    for i in 0..100 {
        now += Duration::from_millis(16);
        let repaint = ui
            .frame(display, now, &mut (), |ui| {
                let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
                Frame::new().id_salt("anim-test").show(ui);
            })
            .repaint_requested();
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
    let _ = ui.frame(display, now, &mut (), |ui| {
        let _ = ui.animate(id, SLOT, 80.0_f32, Some(AnimSpec::SPRING));
        Frame::new().id_salt("anim-novsync").show(ui);
    });

    // Retarget to 400 over a tight loop with 10 µs per frame (NoVsync).
    let mut settled_at = None;
    for i in 0..200_000 {
        now += Duration::from_micros(10);
        let repaint = ui
            .frame(display, now, &mut (), |ui| {
                let _ = ui.animate(id, SLOT, 400.0_f32, Some(AnimSpec::SPRING));
                Frame::new().id_salt("anim-novsync").show(ui);
            })
            .repaint_requested();
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
        .frame(display, Duration::from_millis(16), &mut (), |ui| {
            let v1 = ui.animate(id, SLOT, 7.0_f32, None);
            let v2 = ui.animate(id, SLOT, 9.0_f32, None);
            assert_eq!(v1, 7.0);
            assert_eq!(v2, 9.0);
            Frame::new().id_salt("anim-none").show(ui);
        })
        .repaint_requested();
    assert!(!repaint, "None spec must never request a repaint");
    assert!(
        crate::support::internals::anim_row_count::<f32>(&mut ui) == 0,
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
    let _ = ui.frame(display, Duration::from_millis(0), &mut (), |ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
        Frame::new().id_salt("anim-toggle").show(ui);
    });
    let _ = ui.frame(display, Duration::from_millis(50), &mut (), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Frame::new().id_salt("anim-toggle").show(ui);
    });
    assert!(
        crate::support::internals::anim_row_count::<f32>(&mut ui) > 0,
        "Some(FAST) must allocate a row mid-flight",
    );

    // Frame B: switch to None — the stale row should drop.
    let _ = ui.frame(display, Duration::from_millis(60), &mut (), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, None);
        Frame::new().id_salt("anim-toggle").show(ui);
    });
    assert!(
        crate::support::internals::anim_row_count::<f32>(&mut ui) == 0,
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
    use crate::widgets::theme::{AnimatedLook, TextStyle, WidgetLook};
    use std::cell::Cell;

    let AnimUi {
        mut ui,
        id,
        display,
    } = setup_anim_ui("look-test");

    let bg = Background {
        fill: Color::hex(0x336699).into(),
        stroke: Stroke::solid(Color::hex(0xffffff), 2.0),
        radius: Corners::all(4.0),
        shadow: Shadow::NONE,
    };
    let look = WidgetLook {
        background: Some(bg),
        text: None, // → falls back to TextStyle default
    };
    let fallback = TextStyle::default();

    // None spec: snaps to target, no rows allocated. Use Cell to
    // capture out of the FnMut closure.
    let captured: Cell<Option<AnimatedLook>> = Cell::new(None);
    let _ = ui.frame(display, Duration::from_millis(16), &mut (), |ui| {
        captured.set(Some(look.animate(ui, id, fallback, None)));
        Frame::new().id_salt("look-test").show(ui);
    });
    let snap = captured.get().expect("animate ran");
    assert_eq!(snap.background.fill, bg.fill, "None: fill snaps to target");
    assert_eq!(
        snap.background.stroke.width, 2.0,
        "None: stroke width snaps"
    );
    assert_eq!(snap.background.stroke.brush, bg.stroke.brush);
    assert_eq!(snap.background.radius, bg.radius);
    assert_eq!(
        snap.text.color, fallback.color,
        "None: text falls back to fallback_text",
    );
    assert_eq!(snap.text.font_size_px, fallback.font_size_px);
    assert_eq!(snap.text.line_height_mult, fallback.line_height_mult);
    assert_eq!(
        crate::support::internals::anim_row_count::<AnimatedLook>(&mut ui),
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
            ..bg
        }),
        text: None,
    };
    let _ = ui.frame(display, Duration::from_millis(32), &mut (), |ui| {
        let _ = look2.animate(ui, id, fallback, Some(AnimSpec::FAST));
        Frame::new().id_salt("look-test").show(ui);
    });
    assert!(
        crate::support::internals::anim_row_count::<AnimatedLook>(&mut ui) > 0,
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
        radius: Corners::all(2.0),
        shadow: Shadow::NONE,
    };
    // First touch: snaps current = start, returns settled. No motion
    // started yet.
    let _ = map.tick(id, SLOT, start, AnimSpec::SPRING, 0.016, next_frame());

    // Retarget to a new fill (animated) and a new radius (snap).
    let target = Background {
        fill: Color::rgb(1.0, 0.0, 0.0).into(),
        stroke: Stroke::ZERO,
        radius: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let r = map.tick(id, SLOT, target, AnimSpec::SPRING, 0.016, next_frame());
    assert!(
        !r.settled,
        "spring with a real fill diff must remain in flight after one step",
    );
    assert_eq!(
        r.current.radius, target.radius,
        "snap field must carry target value on the first stepped frame, not lag until settle",
    );
    assert!(
        r.current.fill.as_solid().unwrap().r < target.fill.as_solid().unwrap().r - 0.05,
        "animated fill should still be mid-flight; got {:?}",
        r.current.fill,
    );
}

/// Pin: switching spec from `Spring` to `Duration` mid-flight clears
/// residual spring velocity. Otherwise the next `Duration` frame
/// would compose stale velocity into the segment via the
/// snap-if-close check (which sees nonzero velocity and falls
/// through) plus a fresh lerp segment from the still-moving
/// `current`.
#[test]
fn spec_switch_spring_to_duration_zeros_velocity() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("spec-switch");
    let _ = map.tick(id, SLOT, 0.0_f32, AnimSpec::SPRING, 0.016, next_frame());
    // Build up nonzero velocity by stepping a real spring for a few frames.
    for _ in 0..5 {
        let _ = map.tick(id, SLOT, 1.0_f32, AnimSpec::SPRING, 0.016, next_frame());
    }
    let row = map.rows.get(&(id, SLOT)).expect("row exists mid-spring");
    assert!(
        row.velocity.abs() > 0.01,
        "test setup: spring should have built up velocity by now; got {}",
        row.velocity,
    );

    // Retarget under a Duration spec: velocity must zero out.
    let dur = AnimSpec::Duration {
        secs: 0.1,
        ease: Easing::Linear,
    };
    let _ = map.tick(id, SLOT, 2.0_f32, dur, 0.016, next_frame());
    let row = map.rows.get(&(id, SLOT)).expect("row exists post-switch");
    assert_eq!(
        row.velocity, 0.0,
        "Spring → Duration retarget must zero residual velocity",
    );
}
