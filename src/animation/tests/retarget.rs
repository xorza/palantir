//! A new target mid-flight, including one that switches spec mode.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::animation::tests::support::{SLOT, duration_motion, linear_100ms, spring_velocity, wid};
use crate::primitives::math::domain::internals::assert_close;
use std::time::Duration;

#[test]
fn retarget_mid_flight_starts_new_segment_from_current() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.step(id, SLOT, 0.0, spec, 0.016);
    let r = map.step(id, SLOT, 1.0, spec, 0.016);
    assert_eq!(r.current, 0.0);
    let mid = map.step(id, SLOT, 1.0, spec, 0.05).current;
    assert_eq!(mid, 0.5);

    // In flight: the retarget restarts at 0.5 and spends 50 ms at once: lerp(0.5, 2.0, 0.5).
    let r = map.step(id, SLOT, 2.0, spec, 0.05);
    assert_eq!(r.current, 1.25);
}

/// A target that moves every frame keeps moving; only the first change, from rest, spends nothing.
#[test]
fn a_target_that_moves_every_frame_moves_every_frame() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("follow");
    let spec = linear_100ms();
    let _ = map.step(id, SLOT, 0.0, spec, 0.016);
    let mut last = map.step(id, SLOT, 10.0, spec, 0.016).current;
    assert_eq!(last, 0.0, "the first change starts from rest");
    for frame in 2..=10 {
        let now = map.step(id, SLOT, 10.0 * frame as f32, spec, 0.016).current;
        assert!(now > last, "frame {frame}: {now} after {last}");
        last = now;
    }
}

#[test]
fn spring_to_duration_same_target_restarts_from_current() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("spec-switch");
    let _ = map.step(id, SLOT, 0.0_f32, AnimationSpec::SPRING, 0.016);
    for _ in 0..5 {
        let _ = map.step(id, SLOT, 1.0_f32, AnimationSpec::SPRING, 0.016);
    }
    let row = map.rows.get(&(id, SLOT)).expect("row exists mid-spring");
    let segment_start = row.current();
    let velocity = *spring_velocity(row);
    assert!(
        velocity.abs() > 0.01,
        "test setup: spring should have built up velocity by now; got {velocity}",
    );

    let dur = AnimationSpec::duration(Duration::from_millis(100), Easing::Linear);
    let dt = 0.02;
    let result = map.step(id, SLOT, 1.0_f32, dur, dt);
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
    let duration = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let _ = map.step(id, SLOT, 0.0, duration, 0.016);
    let _ = map.step(id, SLOT, 1.0, duration, 0.016);
    let duration_result = map.step(id, SLOT, 1.0, duration, 0.4);
    assert_eq!(duration_result.current, 0.4);

    // The spring takes over at rest 0.6 short of the target: `1 − 0.6·e^(-13t)(cos t + 13 sin t)` at t = 0.016 (k = 170, h = 13, ω = 1).
    let spring_result = map.step(id, SLOT, 1.0, AnimationSpec::SPRING, 0.016);
    let t = f64::from(0.016_f32);
    let from_point_four = 1.0 - 0.6 * (-13.0 * t).exp() * (t.cos() + 13.0 * t.sin());
    assert_close(
        spring_result.current,
        from_point_four,
        1e-7,
        "the spring starts from the duration's value, to f32 rounding near 0.4",
    );
    let spring_row = map.rows.get(&(id, SLOT)).expect("row exists mid-spring");
    assert!(*spring_velocity(spring_row) > 0.0);

    let segment_start = spring_result.current;
    let dt = 0.25;
    let duration_result = map.step(id, SLOT, 1.0, duration, dt);
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
