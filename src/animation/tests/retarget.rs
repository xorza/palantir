//! A new target mid-flight, including one that switches spec mode.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::easing::Easing;
use crate::animation::tests::support::{SLOT, duration_motion, linear_100ms, spring_velocity, wid};

#[test]
fn retarget_mid_flight_starts_new_segment_from_current() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.step(id, SLOT, 0.0, spec, 0.016);
    // From rest: the change's frame spends none of its 16 ms.
    let r = map.step(id, SLOT, 1.0, spec, 0.016);
    assert_eq!(r.current, 0.0);
    let mid = map.step(id, SLOT, 1.0, spec, 0.05).current;
    // 50 ms of a 100 ms linear segment: progress 0.5, so lerp(0.0, 1.0, 0.5).
    assert_eq!(mid, 0.5);

    // In flight: the retarget restarts the segment at 0.5 and spends its
    // 50 ms at once, half of a linear 100 ms segment: lerp(0.5, 2.0, 0.5).
    let r = map.step(id, SLOT, 2.0, spec, 0.05);
    assert_eq!(r.current, 1.25);
}

/// A target that moves every frame — an animation following a drag —
/// keeps moving on every frame: only the first change, from rest, spends
/// nothing.
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
    let _ = map.step(id, SLOT, 0.0_f32, AnimSpec::SPRING, 0.016);
    for _ in 0..5 {
        let _ = map.step(id, SLOT, 1.0_f32, AnimSpec::SPRING, 0.016);
    }
    let row = map.rows.get(&(id, SLOT)).expect("row exists mid-spring");
    let segment_start = row.current;
    let velocity = *spring_velocity(row);
    assert!(
        velocity.abs() > 0.01,
        "test setup: spring should have built up velocity by now; got {velocity}",
    );

    let dur = AnimSpec::duration(0.1, Easing::Linear);
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
    let duration = AnimSpec::duration(1.0, Easing::Linear);
    let _ = map.step(id, SLOT, 0.0, duration, 0.016);
    let _ = map.step(id, SLOT, 1.0, duration, 0.016);
    let duration_result = map.step(id, SLOT, 1.0, duration, 0.4);
    assert_eq!(duration_result.current, 0.4);

    let spring_result = map.step(id, SLOT, 1.0, AnimSpec::SPRING, 0.016);
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
