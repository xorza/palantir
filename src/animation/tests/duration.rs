//! Fixed-duration tweens: snapping, floors, and finite settling.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::anim_row::MotionRow;
use crate::animation::anim_spec::{AnimMotion, AnimSpec};
use crate::animation::easing::Easing;
use crate::animation::tests::support::{
    AnimUi, SLOT, linear_100ms, next_frame, setup_anim_ui, wid,
};
use crate::common::time::MAX_ANIM_DT;
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
use glam::Vec2;
use std::time::Duration;

/// Through `Ui::animate`, a `Duration { secs = 0 }` spec behaves
/// identically to `None`: snaps to target, drops any in-flight row,
/// no repaint request. Switching from a real spec to instant-Duration
/// resets cleanly so a future real spec starts fresh.
#[test]
fn instant_duration_is_noop_and_drops_row() {
    let instant = Some(AnimSpec::duration(0.0, Easing::Linear));
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
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    let _ = h.at(Duration::from_millis(50)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Block::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    assert!(h.anim_row_count::<f32>() > 0);

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
        let v = ui.animate(id, SLOT, 5.0_f32, Some(AnimSpec::FAST));
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

/// One floor for both motions: a delta of 5e-4 sits above `EPS = 1e-4`,
/// so it animates under a duration and under a spring alike. The
/// retarget frame starts from rest and shows the start value; the next
/// frame moves toward the target without reaching it.
///
/// The spring case is the dark-theme hover the old pixel-scale floor
/// swallowed: `#121212 → #1c1c1c` is about 0.0056 linear a channel, under
/// that floor's 0.01 and well over this one.
#[test]
fn one_floor_animates_a_small_change_under_either_motion() {
    let delta = 5.0e-4_f32;
    let duration = AnimSpec::duration(1.0, Easing::Linear);
    for (label, spec) in [("spring", AnimSpec::SPRING), ("duration", duration)] {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid(label);
        let _ = map.tick(id, SLOT, 0.0, spec, 0.016, next_frame());
        let start = map.tick(id, SLOT, delta, spec, 0.016, next_frame());
        assert_eq!(
            start.current, 0.0,
            "{label}: the change's frame shows the start"
        );
        assert!(!start.settled, "{label}: a change above the floor animates");
        let moving = map.tick(id, SLOT, delta, spec, 0.016, next_frame());
        assert!(
            moving.current > 0.0 && moving.current < delta,
            "{label}: moving toward {delta}, got {}",
            moving.current,
        );
    }

    let mut colours = AnimMapTyped::<RgbaF32>::default();
    let id = wid("hover");
    let (rest, hover) = (RgbaF32::hex(0x121212), RgbaF32::hex(0x1c1c1c));
    let _ = colours.tick(id, SLOT, rest, AnimSpec::SPRING, 0.016, next_frame());
    let _ = colours.tick(id, SLOT, hover, AnimSpec::SPRING, 0.016, next_frame());
    let moving = colours.tick(id, SLOT, hover, AnimSpec::SPRING, 0.016, next_frame());
    assert!(
        !moving.settled && moving.current.r > rest.r && moving.current.r < hover.r,
        "the hover fades over several frames, got {:?}",
        moving.current,
    );
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
    let _ = map.tick(id, SLOT, 0.0, spec, 0.016, next_frame());
    // From rest: the change's frame spends none of its 16 ms.
    let r = map.tick(id, SLOT, 1.0, spec, 0.016, next_frame());
    assert_eq!(r.current, 0.0);
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
    // The change's frame spends nothing, then 600 steps of 0.1 s run the
    // 60 s; one more absorbs the f32 sum landing a hair under 60.
    let mut settled = None;
    for step in 0..=601 {
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
        "maximum duration did not settle after 60.1 seconds of steps",
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

/// `OutBack` overshoots its target before it settles, however small the
/// change: a 0.01 move over 0.2 s at 60 fps passes 0.01 on its way out.
/// Checked mid-curve, the snap floor caught the frame where the curve
/// crossed the target and ended the motion there.
#[test]
fn out_back_reaches_its_overshoot_on_a_small_change() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("out-back");
    let spec = AnimSpec::duration(0.2, Easing::OutBack);
    let _ = map.tick(id, SLOT, 0.0, spec, 0.016, next_frame());
    let mut peak = 0.0_f32;
    for _ in 0..30 {
        let r = map.tick(id, SLOT, 0.01, spec, 1.0 / 60.0, next_frame());
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
#[cfg(debug_assertions)]
#[test]
fn a_non_finite_target_is_refused() {
    for target in [f32::NAN, f32::INFINITY] {
        let mut map = AnimMapTyped::<f32>::default();
        let id = wid("nan");
        let _ = map.tick(id, SLOT, 0.0, AnimSpec::FAST, 0.016, next_frame());
        crate::common::panic_probe::assert_panics_with("is not finite", || {
            map.tick(id, SLOT, target, AnimSpec::FAST, 0.016, next_frame())
        });
    }
}
