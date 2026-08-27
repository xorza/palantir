//! Fixed-duration tweens: snapping, floors, and finite settling.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::anim_row::MotionRow;
use crate::animation::anim_spec::{AnimMotion, AnimSpec};
use crate::animation::easing::Easing;
use crate::animation::tests::support::{
    AnimUi, SLOT, linear_100ms, next_frame, setup_anim_ui, wid,
};
use crate::common::time::MAX_ANIM_DT;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::widgets::frame::Frame;
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
    let repaint = h
        .at(Duration::from_millis(0))
        .frame(|ui| {
            let v = ui.animate(id, SLOT, 1.0_f32, instant);
            assert_eq!(v, 1.0);
            Frame::new()
                .id(WidgetId::from_hash("anim-instant"))
                .show(ui);
        })
        .repaint_requested;
    assert!(!repaint);
    assert_eq!(h.anim_row_count::<f32>(), 0);

    // Mid-flight on FAST: row gets allocated.
    let _ = h.at(Duration::from_millis(0)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
        Frame::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    let _ = h.at(Duration::from_millis(50)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Frame::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    assert!(h.anim_row_count::<f32>() > 0);

    // Switching to instant mid-flight: snap and drop.
    let _ = h.at(Duration::from_millis(60)).frame(|ui| {
        let v = ui.animate(id, SLOT, 1.0_f32, instant);
        assert_eq!(v, 1.0);
        Frame::new()
            .id(WidgetId::from_hash("anim-instant"))
            .show(ui);
    });
    assert_eq!(
        h.anim_row_count::<f32>(),
        0,
        "instant must drop the stale row inserted by FAST",
    );

    // Switching back to FAST with a new target: first-touch snaps.
    let _ = h.at(Duration::from_millis(70)).frame(|ui| {
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
