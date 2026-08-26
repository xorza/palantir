//! A new target mid-flight, including one that switches spec mode.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::easing::Easing;
use crate::animation::tests::support::{
    SLOT, duration_motion, linear_100ms, next_frame, spring_velocity, wid,
};

#[test]
fn retarget_mid_flight_starts_new_segment_from_current() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, 0.0, spec, 0.016, next_frame());
    let _ = map.tick(id, SLOT, 1.0, spec, 0.0, next_frame());
    let mid = map.tick(id, SLOT, 1.0, spec, 0.05, next_frame()).current;
    // 50 ms of a 100 ms linear segment: progress 0.5, so lerp(0.0, 1.0, 0.5).
    assert_eq!(mid, 0.5);

    let r = map.tick(id, SLOT, 2.0, spec, 0.0, next_frame());
    assert_eq!(r.current, mid, "retarget must preserve current");
    let r = map.tick(id, SLOT, 2.0, spec, 0.05, next_frame());
    // The retarget restarted the segment at 0.5, so another half of a
    // linear 100 ms segment gives lerp(0.5, 2.0, 0.5).
    assert_eq!(r.current, 1.25);
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
