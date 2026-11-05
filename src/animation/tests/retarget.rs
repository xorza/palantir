//! A new target mid-flight, including one that switches spec mode, and a spec that changes
//! with no new target.

use crate::animation::animation_key::AnimationKey;
use crate::animation::animation_map_typed::AnimationMapTyped;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::animation::tests::support::{self, SLOT};
use crate::primitives::math::domain;
use std::time::Duration;

#[test]
fn retarget_mid_flight_starts_new_segment_from_current() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("a");
    let spec = support::linear_100ms();
    let _ = map.step(AnimationRequest::new(id, SLOT, 0.0), spec, 0.016);
    let r = map.step(AnimationRequest::new(id, SLOT, 1.0), spec, 0.016);
    assert_eq!(r.value, 0.0);
    let mid = map
        .step(AnimationRequest::new(id, SLOT, 1.0), spec, 0.05)
        .value;
    assert_eq!(mid, 0.5);

    // In flight: the retarget restarts at 0.5 and spends 50 ms at once: lerp(0.5, 2.0, 0.5).
    let r = map.step(AnimationRequest::new(id, SLOT, 2.0), spec, 0.05);
    assert_eq!(r.value, 1.25);
}

/// A target that moves every frame keeps moving; only the first change, from rest, spends nothing.
#[test]
fn a_target_that_moves_every_frame_moves_every_frame() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("follow");
    let spec = support::linear_100ms();
    let _ = map.step(AnimationRequest::new(id, SLOT, 0.0), spec, 0.016);
    let mut last = map
        .step(AnimationRequest::new(id, SLOT, 10.0), spec, 0.016)
        .value;
    assert_eq!(last, 0.0, "the first change starts from rest");
    for frame in 2..=10 {
        let now = map
            .step(
                AnimationRequest::new(id, SLOT, 10.0 * frame as f32),
                spec,
                0.016,
            )
            .value;
        assert!(now > last, "frame {frame}: {now} after {last}");
        last = now;
    }
}

/// A spring in flight runs on under its own spec when the spec turns to a duration with no new
/// target, step for step as a twin under the spring, velocity included. The next target takes
/// the duration from the value shown: `2 + (start − 2)·(1 − 0.02 / 0.1)`.
#[test]
fn a_new_model_waits_for_the_next_target() {
    let mut map = AnimationMapTyped::<f32>::default();
    let mut twin = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("spec-switch");
    for (target, frames) in [(0.0_f32, 1), (1.0, 5)] {
        for _ in 0..frames {
            let _ = map.step(
                AnimationRequest::new(id, SLOT, target),
                AnimationSpec::SPRING,
                0.016,
            );
            let _ = twin.step(
                AnimationRequest::new(id, SLOT, target),
                AnimationSpec::SPRING,
                0.016,
            );
        }
    }
    let dur = AnimationSpec::duration(Duration::from_millis(100), Easing::Linear);
    for frame in 0..3 {
        let kept = map.step(AnimationRequest::new(id, SLOT, 1.0), dur, 0.016);
        let spring = twin.step(
            AnimationRequest::new(id, SLOT, 1.0),
            AnimationSpec::SPRING,
            0.016,
        );
        assert_eq!(kept.value, spring.value, "frame {frame}");
        assert_eq!(
            support::spring_velocity(&map.rows[&(id, SLOT)]),
            support::spring_velocity(&twin.rows[&(id, SLOT)]),
            "frame {frame}",
        );
    }
    let row = &map.rows[&(id, SLOT)];
    let segment_start = row.cur();
    let velocity = *support::spring_velocity(row);
    assert!(
        velocity.abs() > 0.01,
        "test setup: spring should have built up velocity by now; got {velocity}",
    );

    let dt = 0.02;
    let result = map.step(AnimationRequest::new(id, SLOT, 2.0), dur, dt);
    let motion = support::duration_motion(&map.rows[&(id, SLOT)]);
    assert_eq!(*motion.start_offset, segment_start - 2.0);
    assert_eq!(motion.elapsed, dt);
    assert_eq!(result.value, 2.0 + (segment_start - 2.0) * (1.0 - dt / 0.1));
}

/// Each new target takes the spec it comes with and starts that model from the value shown: a
/// duration at 0.375 of the way to 1, a spring from rest toward 2, a duration again toward 3.
#[test]
fn each_target_starts_the_model_it_comes_with() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("round-trip-spec-switch");
    let duration = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let _ = map.step(AnimationRequest::new(id, SLOT, 0.0), duration, 0.016);
    let _ = map.step(AnimationRequest::new(id, SLOT, 1.0), duration, 0.016);
    // A binary fraction, so the offset `-1 · (1 − 0.375)` from the target is exact.
    let duration_result = map.step(AnimationRequest::new(id, SLOT, 1.0), duration, 0.375);
    assert_eq!(duration_result.value, 0.375);

    // The spring starts at rest 1.625 short of 2: `2 − 1.625·e^(-13t)(cos t + 13 sin t)` at
    // t = 0.016 (k = 170, h = 13, ω = 1).
    let spring_result = map.step(
        AnimationRequest::new(id, SLOT, 2.0),
        AnimationSpec::SPRING,
        0.016,
    );
    let t = f64::from(0.016_f32);
    let from_rest = 2.0 - 1.625 * (-13.0 * t).exp() * (t.cos() + 13.0 * t.sin());
    domain::internals::assert_close(
        spring_result.value,
        from_rest,
        2.4e-7,
        "the spring starts from the duration's value, to two f32 ulps (2 · 2⁻²³) of the offset near 1.6",
    );
    let spring_row = &map.rows[&(id, SLOT)];
    assert!(*support::spring_velocity(spring_row) > 0.0);

    let segment_start = spring_result.value;
    let dt = 0.25;
    let duration_result = map.step(AnimationRequest::new(id, SLOT, 3.0), duration, dt);
    let motion = support::duration_motion(&map.rows[&(id, SLOT)]);
    assert_eq!(*motion.start_offset, segment_start - 3.0);
    assert_eq!(motion.elapsed, dt);
    assert_eq!(
        duration_result.value,
        3.0 + (segment_start - 3.0) * (1.0 - dt)
    );
}

/// CSS Transitions' faster reversing: a segment turned back to where it came from runs over the
/// share it covered. Linear 1 s from 0 to 1, reversed at 0.5 s: back to 0 in 0.5 s. Reversed again
/// 0.25 s in (at 0.25, half way back): the shortening is `0.5 · 0.5 + 1 − 0.5 = 0.75`, so on to 1
/// in 0.75 s. Times are binary fractions, so every value is exact.
#[test]
fn a_reversal_runs_over_the_share_it_covered() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("reverse");
    let spec = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let mut step = |target: f32, dt: f32| {
        map.step(AnimationRequest::new(id, SLOT, target), spec, dt)
            .value
    };
    let _ = step(0.0, 0.0);
    assert_eq!(step(1.0, 0.0), 0.0, "the first change starts from rest");
    assert_eq!(step(1.0, 0.5), 0.5);

    assert_eq!(step(0.0, 0.25), 0.25, "lerp(0.5, 0, 0.25 / 0.5)");
    assert_eq!(step(1.0, 0.375), 0.625, "lerp(0.25, 1, 0.375 / 0.75)");
    assert_eq!(step(1.0, 0.375), 1.0, "0.75 s in, at the end");

    // A reversal after a segment that ran to its end shortens nothing: the whole second.
    assert_eq!(step(0.0, 0.0), 1.0, "from rest");
    assert_eq!(step(0.0, 0.5), 0.5);
    // A target that is not where the segment came from runs the whole length too.
    assert_eq!(step(2.0, 0.5), 0.5 + (2.0 - 0.5) * 0.5);
}

/// A new target under the request's key is followed, not animated toward. Linear 1 s:
///
/// - At rest under key 0, a target moved from 0 to 4 lands at once and stays settled.
/// - Key 1 heads for 8: from rest at 4 the first frame spends nothing, and 0.25 s later the offset
///   `4 − 8 = −4` has faded to `−3`, so 5.
/// - Under key 1 the target moves to 16: the offset and its clock carry on, 0.5 s in it is `−2`, so
///   14. A row with no key would restart from 5 toward 16, at 7.75.
/// - 1 s in it lands on 16. Every value is a binary fraction, so exact.
#[test]
fn a_target_moving_under_its_key_is_followed() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("keyed");
    let spec = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let mut step = |target: f32, key: u8, dt: f32| {
        map.step(
            AnimationRequest::new(id, SLOT, target).with_key(AnimationKey::of(key)),
            spec,
            dt,
        )
    };
    let _ = step(0.0, 0, 0.0);
    let moved = step(4.0, 0, 0.25);
    assert_eq!(
        (moved.value, moved.settled),
        (4.0, true),
        "followed at rest"
    );

    assert_eq!(step(8.0, 1, 0.25).value, 4.0, "a new key starts from rest");
    assert_eq!(step(8.0, 1, 0.25).value, 5.0);
    assert_eq!(
        step(16.0, 1, 0.25).value,
        14.0,
        "the offset rides the moved target"
    );
    let landed = step(16.0, 1, 0.5);
    assert_eq!((landed.value, landed.settled), (16.0, true));
}

/// A keyed row turns back by key, so a reversal holds while the target moves. Linear 1 s, key 0 at
/// 0, key 1 heads for 1 and is at 0.5 after 0.5 s. Back to key 0, whose target has moved to 0.25:
/// the segment came from key 0, so it runs over the half it covered, and 0.25 s in it is half way
/// from 0.5 to 0.25, at 0.375. Compared by value, 0.25 is not where it came from, so it would run
/// the whole second, at 0.4375. Binary fractions, so exact.
#[test]
fn a_reversal_by_key_holds_while_the_target_moves() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("keyed-reverse");
    let spec = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let mut step = |target: f32, key: u8, dt: f32| {
        map.step(
            AnimationRequest::new(id, SLOT, target).with_key(AnimationKey::of(key)),
            spec,
            dt,
        )
        .value
    };
    let _ = step(0.0, 0, 0.0);
    assert_eq!(step(1.0, 1, 0.0), 0.0, "from rest");
    assert_eq!(step(1.0, 1, 0.5), 0.5);
    assert_eq!(step(0.25, 0, 0.25), 0.375, "back over the half it covered");
    assert_eq!(step(0.25, 0, 0.25), 0.25, "0.5 s back, at its end");
}

/// A spring under its key rides a moved target: once key 1 has sprung part way from 0 toward 1,
/// its target moves to 3 and the offset from it stays, so an advance of no time moves the value by
/// exactly the target's step of 2; then it settles on 3.
#[test]
fn a_spring_rides_a_target_moving_under_its_key() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("keyed-spring");
    let mut step = |target: f32, key: u8, dt: f32| {
        map.step(
            AnimationRequest::new(id, SLOT, target).with_key(AnimationKey::of(key)),
            AnimationSpec::SPRING,
            dt,
        )
    };
    let _ = step(0.0, 0, 0.0);
    assert_eq!(step(1.0, 1, 0.016).value, 0.0, "from rest");
    let before = step(1.0, 1, 0.016).value;
    assert!(0.0 < before && before < 1.0, "in flight at {before}");
    let after = step(3.0, 1, 0.0).value;
    assert_eq!(
        after - before,
        2.0,
        "an advance of no time moves only with the target"
    );
    let mut last = step(3.0, 1, 0.016);
    while !last.settled {
        last = step(3.0, 1, 0.016);
    }
    assert_eq!(last.value, 3.0);
}

/// A resting row keeps nothing of the run before it, so a change too small to animate does not
/// make the next one a turn back. Under a 1 s linear spec, 0 under key 0, then `tiny` under key 1,
/// then 16 under key 0 rests on `tiny` for its first frame, as a motion from rest spends nothing
/// there, and at 0.25 s has `0.75` of its offset left: not 16 at once.
#[test]
fn a_change_too_small_to_animate_does_not_shorten_the_next() {
    let mut map = AnimationMapTyped::<f32>::default();
    let id = support::widget_id("small then far");
    let spec = AnimationSpec::duration(Duration::from_secs(1), Easing::Linear);
    let mut step = |target: f32, key: u8, dt: f32| {
        map.step(
            AnimationRequest::new(id, SLOT, target).with_key(AnimationKey::of(key)),
            spec,
            dt,
        )
    };
    let tiny = 2.0_f32.powi(-20);
    let _ = step(0.0, 0, 0.0);
    let small = step(tiny, 1, 0.25);
    assert_eq!((small.value, small.settled), (tiny, true));
    let far = step(16.0, 0, 0.25);
    assert_eq!((far.value, far.settled), (tiny, false));
    let quarter = step(16.0, 0, 0.25);
    assert_eq!(
        (quarter.value, quarter.settled),
        (16.0 + (tiny - 16.0) * 0.75, false)
    );
}
