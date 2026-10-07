//! Fixed-duration tweens: snapping, floors, and finite settling.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::anim_row::MotionRow;
use crate::animation::animation_spec::{AnimMotion, AnimationSpec};
use crate::animation::easing::Easing;
use crate::animation::tests::support::{AnimUi, SLOT, linear_100ms, setup_anim_ui, wid};
use crate::common::time::MAX_ANIM_DT;
use crate::internals::panic_probe;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::domain::internals::assert_close;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::Vec2;
use std::time::Duration;

/// A zero-second `Duration` spec behaves like `None` through `Ui::animate`: it snaps, drops any in-flight row.
#[test]
fn instant_duration_is_noop_and_drops_row() {
    let instant = Some(AnimationSpec::duration(Duration::ZERO, Easing::Linear));
    let AnimUi { mut h, id } = setup_anim_ui("anim-instant");

    let passes = h.at(Duration::from_millis(0)).frame_passes(|ui| {
        let v = ui.animate(id, SLOT, 1.0_f32, instant);
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
        v
    });
    assert_eq!(*passes.a(), 1.0);
    assert!(!passes.report().repaint_requested);
    assert_eq!(h.anim_row_count::<f32>(), 0);

    let _ = h.at(Duration::from_millis(0)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimationSpec::FAST));
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    let _ = h.at(Duration::from_millis(50)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimationSpec::FAST));
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    assert_eq!(h.anim_row_count::<f32>(), 1);

    let v = h.at(Duration::from_millis(60)).frame_value(|ui| {
        let v = ui.animate(id, SLOT, 1.0_f32, instant);
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
        v
    });
    assert_eq!(v, 1.0);
    assert_eq!(
        h.anim_row_count::<f32>(),
        0,
        "instant must drop the stale row inserted by FAST",
    );

    let v = h.at(Duration::from_millis(70)).frame_value(|ui| {
        let v = ui.animate(id, SLOT, 5.0_f32, Some(AnimationSpec::FAST));
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
        v
    });
    assert_eq!(v, 5.0, "post-instant first-touch snaps to new target");
}

/// Sub-perceptual drift snaps instead of animating, using the type's own settle tolerance: `1e-5` on an
/// `f32` (1e-4) snaps; `2e-4` snaps on a colour channel (under `1/4096`) but animates on an `f32`.
#[test]
fn target_below_snap_floor_snaps_without_animating() {
    let duration = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let tiny = 1.0e-5;
    let cases: &[(&str, AnimationSpec)] =
        &[("duration", duration), ("spring", AnimationSpec::SPRING)];
    for (label, spec) in cases {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid("a");
        let _ = map.step(id, SLOT, 0.0, *spec, 0.016);
        let r = map.step(id, SLOT, tiny, *spec, 0.016);
        assert_eq!(
            r.current, tiny,
            "case {label}: snap-if-close must reach new target exactly",
        );
        assert!(
            r.settled,
            "case {label}: sub-eps drift must report settled (no repaint)",
        );

        let mut scalars = AnimMapTyped::<f32>::default();
        let _ = scalars.step(id, SLOT, 0.5, *spec, 0.016);
        let r = scalars.step(id, SLOT, 0.5 + 2e-4, *spec, 0.016);
        assert!(!r.settled, "case {label}: 2e-4 on an f32 animates");

        let mut colours = AnimMapTyped::<RgbaF32>::default();
        let (grey, nudged) = (
            RgbaF32::new(0.5, 0.5, 0.5, 1.0),
            RgbaF32::new(0.5 + 2e-4, 0.5, 0.5, 1.0),
        );
        let _ = colours.step(id, SLOT, grey, *spec, 0.016);
        let r = colours.step(id, SLOT, nudged, *spec, 0.016);
        assert_eq!(r.current, nudged, "case {label}: a colour lands on target");
        assert!(r.settled, "case {label}: 2e-4 on a colour snaps");
    }
}

/// One tolerance for both motions: 5e-4 exceeds an `f32`'s 1e-4, so it animates under a duration and a
/// spring; the retarget frame shows the start value, the next moves toward the target. The spring case is
/// a dark-theme hover (`#121212 → #1c1c1c`, ~0.0056 linear per channel).
#[test]
fn one_floor_animates_a_small_change_under_either_motion() {
    let delta = 5.0e-4_f32;
    let duration = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let t = f64::from(0.016f32);
    let spring_travel = 1.0 - (-13.0 * t).exp() * (t.cos() + 13.0 * t.sin());
    for (label, spec) in [("spring", AnimationSpec::SPRING), ("duration", duration)] {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid(label);
        let _ = map.step(id, SLOT, 0.0, spec, 0.016);
        let start = map.step(id, SLOT, delta, spec, 0.016);
        assert_eq!(
            start.current, 0.0,
            "{label}: the change's frame shows the start"
        );
        assert!(!start.settled, "{label}: a change above the floor animates");
        let moving = map.step(id, SLOT, delta, spec, 0.016);
        // Linear: 0.016 of the way over 1 s. Spring from rest: `delta·(1 − e^(-13t)(cos t + 13 sin t))` at t = 0.016.
        let expected = match label {
            "duration" => f64::from(delta * 0.016),
            _ => f64::from(delta) * spring_travel,
        };
        assert_close(
            moving.current,
            expected,
            1e-10,
            "the f32 step's coefficients are each within an ulp of the \
             closed form's, ~1e-7 of delta",
        );
    }

    let mut colours = AnimMapTyped::<RgbaF32>::default();
    let id = wid("hover");
    let (rest, hover) = (RgbaF32::hex(0x121212), RgbaF32::hex(0x1c1c1c));
    let _ = colours.step(id, SLOT, rest, AnimationSpec::SPRING, 0.016);
    let _ = colours.step(id, SLOT, hover, AnimationSpec::SPRING, 0.016);
    let moving = colours.step(id, SLOT, hover, AnimationSpec::SPRING, 0.016);
    assert!(!moving.settled, "the hover fades over several frames");
    let expected = f64::from(rest.r) + f64::from(hover.r - rest.r) * spring_travel;
    assert_close(
        moving.current.r,
        expected,
        1e-8,
        "the first step travels the closed form's share, to f32 rounding \
         of a value near 0.007",
    );
}

#[test]
fn first_touch_returns_target_and_settled() {
    for (label, spec) in [
        ("duration", AnimationSpec::FAST),
        ("spring", AnimationSpec::SPRING),
    ] {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid(label);
        let r = map.step(id, SLOT, 1.0, spec, 0.016);
        assert_eq!(r.current, 1.0, "{label}: first touch must snap");
        assert!(r.settled, "{label}: first touch must report settled");
        let row = &map.rows[&(id, SLOT)];
        match &row.motion {
            MotionRow::Duration {
                segment_start,
                elapsed,
                ..
            } => {
                assert_eq!((*segment_start, *elapsed), (1.0, 0.0));
                assert!(matches!(spec.motion, AnimMotion::Duration { .. }));
            }
            MotionRow::Spring { velocity, .. } => {
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
    let _ = map.step(id, SLOT, 0.0, spec, 0.016);
    let r = map.step(id, SLOT, 1.0, spec, 0.016);
    assert_eq!(r.current, 0.0);
    let r = map.step(id, SLOT, 1.0, spec, 0.05);
    assert_eq!(r.current, 0.05 / 0.1, "linear: elapsed over secs");
    assert!(!r.settled, "halfway is not settled");
    let r = map.step(id, SLOT, 1.0, spec, 0.05);
    assert_eq!(r.current, 1.0, "must snap to target on settle");
    assert!(r.settled, "100ms total elapsed must settle");

    let mut boundary_map = AnimMapTyped::<f32>::default();
    let boundary_id = wid("maximum-duration");
    let boundary = AnimationSpec::duration(Duration::from_secs(60), Easing::Linear);
    let _ = boundary_map.step(boundary_id, SLOT, 0.0, boundary, 0.0);
    // The change's frame spends nothing, then 600 steps of 0.1 s run the 60 s; one more absorbs f32 rounding.
    let mut settled = None;
    for step in 0..=601 {
        let result = boundary_map.step(boundary_id, SLOT, 1.0, boundary, MAX_ANIM_DT);
        assert!(result.current.is_finite());
        if result.settled {
            assert_eq!(result.current, 1.0);
            settled = Some(step);
            break;
        }
    }
    assert_eq!(settled, Some(601));
}

#[test]
fn dt_zero_does_not_advance_duration() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.step(id, SLOT, 0.0, spec, 0.0);
    let _ = map.step(id, SLOT, 1.0, spec, 0.0);
    let r = map.step(id, SLOT, 1.0, spec, 0.0);
    assert_eq!(r.current, 0.0, "dt=0 must not advance toward target");
    assert!(!r.settled, "still in flight");
}

#[test]
fn vec2_duration_lerps_componentwise() {
    let mut map = AnimMapTyped::<Vec2>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.step(id, SLOT, Vec2::ZERO, spec, 0.0);
    let _ = map.step(id, SLOT, Vec2::new(10.0, 20.0), spec, 0.0);
    let r = map.step(id, SLOT, Vec2::new(10.0, 20.0), spec, 0.05);
    let progress = 0.05 / 0.1;
    assert_eq!(
        r.current,
        Vec2::new(10.0 * progress, 20.0 * progress),
        "each component lerps by the same linear progress",
    );
}

/// `OutBack` overshoots its target before settling, however small the change; a snap floor checked
/// mid-curve would end the motion at the crossing frame.
#[test]
fn out_back_reaches_its_overshoot_on_a_small_change() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("out-back");
    let spec = AnimationSpec::duration(Duration::from_millis(200), Easing::OutBack);
    let _ = map.step(id, SLOT, 0.0, spec, 0.016);
    let mut peak = 0.0_f32;
    for _ in 0..30 {
        let r = map.step(id, SLOT, 0.01, spec, 1.0 / 60.0);
        peak = peak.max(r.current);
        if r.settled {
            assert_eq!(r.current, 0.01);
            break;
        }
    }
    assert!(peak > 0.0101, "no overshoot past 0.01, peak {peak}");
}

/// A non-finite target is a caller logic error: NaN != NaN would retarget every frame forever.
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "probes a debug_assert!, which release compiles out"
)]
fn a_non_finite_target_is_refused() {
    for target in [f32::NAN, f32::INFINITY] {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid("nan");
        let _ = map.step(id, SLOT, 0.0, AnimationSpec::FAST, 0.016);
        panic_probe::assert_panics_with("is not finite", || {
            map.step(id, SLOT, target, AnimationSpec::FAST, 0.016)
        });
    }
}
