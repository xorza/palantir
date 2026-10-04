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

/// Through `Ui::animate`, a `Duration { secs = 0 }` spec behaves
/// identically to `None`: snaps to target, drops any in-flight row,
/// no repaint request. Switching from a real spec to instant-Duration
/// resets cleanly so a future real spec starts fresh.
#[test]
fn instant_duration_is_noop_and_drops_row() {
    let instant = Some(AnimationSpec::duration(Duration::ZERO, Easing::Linear));
    let AnimUi { mut h, id } = setup_anim_ui("anim-instant");

    // Instant on a fresh slot: snaps, no row, no repaint.
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

    // Mid-flight on FAST: row gets allocated.
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

    // Switching to instant mid-flight: snap and drop.
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

    // Switching back to FAST with a new target: first-touch snaps.
    let v = h.at(Duration::from_millis(70)).frame_value(|ui| {
        let v = ui.animate(id, SLOT, 5.0_f32, Some(AnimationSpec::FAST));
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
        v
    });
    assert_eq!(v, 5.0, "post-instant first-touch snaps to new target");
}

/// Sub-perceptual drift between `target` and `current` must snap rather
/// than starting a full ease/spring cycle. Otherwise tiny float
/// quantization in the caller (rounded theme colors, sub-pixel rect
/// drift) would spuriously request repaints frame after frame for
/// changes the user can't see. Both specs snap under the type's own
/// settle tolerance, so `1e-5` on an `f32` (tolerance 1e-4) snaps on both.
///
/// The tolerance is the type's: `2e-4` on one channel is under a colour's
/// `1/4096 ≈ 2.44e-4` and snaps, while the same `2e-4` on an `f32` is
/// over its 1e-4 and animates.
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

/// One tolerance for both motions: a delta of 5e-4 sits above an `f32`'s
/// 1e-4, so it animates under a duration and under a spring alike. The
/// retarget frame starts from rest and shows the start value; the next
/// frame moves toward the target without reaching it.
///
/// The spring case is a dark-theme hover: `#121212 → #1c1c1c` is about
/// 0.0056 linear a channel, under a pixel-scale floor of 0.01 and well
/// over a colour's 1/4096.
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
        // Linear over 1 s: 0.016 of the way. The spring, released from
        // rest: `delta·(1 − e^(-13t)(cos t + 13 sin t))` at t = 0.016, in
        // f64.
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
    // From rest: the change's frame spends none of its 16 ms.
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
    // The change's frame spends nothing, then 600 steps of 0.1 s run the
    // 60 s; one more absorbs the f32 sum landing a hair under 60.
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

/// `OutBack` overshoots its target before it settles, however small the
/// change: a 0.01 move over 0.2 s at 60 fps passes 0.01 on its way out.
/// Checked mid-curve, the snap floor caught the frame where the curve
/// crossed the target and ended the motion there.
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

/// A non-finite target is a caller's logic error: NaN differs from itself,
/// so the row would retarget every frame and never settle.
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
