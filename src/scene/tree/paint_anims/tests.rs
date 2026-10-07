use crate::common::hash::Hasher;
use crate::primitives::math::domain::internals::assert_close;
use crate::scene::tree::paint_anims::paint_animation::{PaintChannel, PaintCurve, PaintRepeat};
use crate::scene::tree::paint_anims::*;
use std::f32::consts::TAU;
use std::hash::Hasher as _;

const HP: Duration = Duration::from_millis(500);
const START: Duration = Duration::from_secs(1);
/// Long enough that no test reaches it, isolating phase from the settle.
const NO_STOP: Duration = Duration::MAX;

/// A blink that runs forever, for the cases about phase alone.
fn blink() -> PaintAnimation {
    PaintAnimation::alpha(0.0, 1.0)
        .with_started_at(START)
        .with_period(HP * 2)
        .with_steps(2)
        .with_repeat(PaintRepeat::Settle(NO_STOP))
        .with_curve(curves::square)
}

fn spinning(shape_idx: u32, speed: f32) -> PaintAnimEntry {
    PaintAnimEntry {
        anim: PaintAnimation::turn(0.0, 1.0)
            .with_started_at(START)
            .with_period(Duration::from_secs_f32(TAU / speed))
            .with_repeat(PaintRepeat::Forever)
            .with_curve(curves::linear),
        shape_idx,
        row: 0,
        node: NodeId(0),
    }
}

#[test]
fn sparse_cursor_samples_boundaries_and_advances_across_skipped_animations() {
    const LAST_SHAPE: u32 = 1_000_000;
    let mut anims = PaintAnims::default();
    anims.push_entry(spinning(0, 1.0));
    anims.push_entry(spinning(5, 2.0));
    anims.push_entry(spinning(10, 3.0));
    anims.push_entry(spinning(LAST_SHAPE, 4.0));

    let registered: Vec<u32> = anims.entries.iter().map(|entry| entry.shape_idx).collect();
    assert_eq!(registered, [0, 5, 10, LAST_SHAPE]);

    let now = START + Duration::from_secs(1);
    let mut cursor = anims.cursor();
    assert_eq!(cursor.sample(0, now).rotation, 1.0);
    assert_eq!(cursor.sample(1, now), PaintMod::IDENTITY);
    assert_eq!(cursor.sample(5, now).rotation, 2.0);
    assert_eq!(
        cursor.sample(LAST_SHAPE, now).rotation,
        4.0,
        "jumping over culled shape 10 must not strand the cursor",
    );

    // A jump between two registrations: culling skipped shape 5, and shape 6 sits
    // below the next registered index. It owns no animation; taking 10's would
    // misparent it and leave shape 10 unanimated.
    let mut cursor = anims.cursor();
    assert_eq!(cursor.sample(0, now).rotation, 1.0);
    assert_eq!(
        cursor.sample(6, now),
        PaintMod::IDENTITY,
        "a shape between registrations owns no animation",
    );
    assert_eq!(
        cursor.sample(10, now).rotation,
        3.0,
        "the overshot registration must still be there for its own shape",
    );

    let entry_capacity = anims.entries.capacity();
    anims.clear();
    assert!(anims.entries.is_empty());
    assert_eq!(anims.entries.capacity(), entry_capacity);
}

/// The reading half of the ordering contract `push_entry` asserts. Debug-only:
/// the check needs a field the release cursor lacks.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "must be monotonic")]
fn sampling_backwards_is_a_caller_bug() {
    let mut anims = PaintAnims::default();
    anims.push_entry(spinning(2, 1.0));
    let now = START + Duration::from_secs(1);
    let mut cursor = anims.cursor();
    cursor.sample(4, now);
    cursor.sample(3, now);
}

#[test]
fn blink_solid_at_start() {
    let a = blink();
    assert_eq!(a.sample(START).alpha, 1.0);
}

#[test]
fn blink_flips_at_first_boundary() {
    let a = blink();
    // Just before the boundary: still solid.
    let before = START + HP - Duration::from_micros(1);
    assert_eq!(a.sample(before).alpha, 1.0);
    // At the boundary: hidden.
    let at = START + HP;
    assert_eq!(a.sample(at).alpha, 0.0);
    // Two boundaries later: solid again.
    let two = START + HP + HP;
    assert_eq!(a.sample(two).alpha, 1.0);
}

#[test]
fn next_wake_aligns_with_next_boundary() {
    let a = blink();
    // Mid-phase: wake at the next half-period boundary.
    assert_eq!(
        a.next_wake(START + Duration::from_millis(100)),
        Some(START + HP),
    );
    // On the boundary: wake at the *next* one (strictly future).
    assert_eq!(a.next_wake(START + HP), Some(START + HP + HP));
    // Several periods in.
    assert_eq!(
        a.next_wake(START + HP + HP + Duration::from_millis(50)),
        Some(START + HP + HP + HP),
    );
}

#[test]
fn pre_start_phase_is_solid_and_wakes_at_start() {
    let a = blink();
    let before = START - Duration::from_millis(200);
    assert_eq!(a.sample(before).alpha, 1.0);
    assert_eq!(a.next_wake(before), Some(START));
}

/// A zero period finishes each pass at once. With linear `0 -> 1` alpha the
/// sampled alpha is the phase: `Once` jumps to its end value, a repeat stays at
/// its start, `Settle` stops modifying the shape (alpha 1) at its settle time.
/// Before the start every mode reads phase 0 and wakes at the start.
#[test]
fn a_zero_period_settles_to_the_right_value() {
    const SETTLE: Duration = Duration::from_secs(2);
    let before = START - Duration::from_millis(200);
    let settled = START + SETTLE;
    let cases = [
        (
            PaintRepeat::Once,
            [(0.0, Some(START)), (1.0, None), (1.0, None)],
        ),
        (
            PaintRepeat::Forever,
            [(0.0, Some(START)), (0.0, None), (0.0, None)],
        ),
        (
            PaintRepeat::Settle(SETTLE),
            [(0.0, Some(START)), (0.0, Some(settled)), (1.0, None)],
        ),
    ];
    for (repeat, expected) in cases {
        let a = PaintAnimation::alpha(0.0, 1.0)
            .with_started_at(START)
            .with_period(Duration::ZERO)
            .with_repeat(repeat)
            .with_curve(curves::linear);
        for (now, (alpha, wake)) in [before, START, settled].into_iter().zip(expected) {
            assert_eq!(
                (a.sample(now).alpha, a.next_wake(now)),
                (alpha, wake),
                "{repeat:?} at {now:?}",
            );
        }
    }
}

#[test]
fn spin_angle_is_elapsed_times_speed_wrapped() {
    let speed = 4.0; // rad/s
    let a = PaintAnimation::turn(0.0, 1.0)
        .with_started_at(START)
        .with_period(Duration::from_secs_f32(TAU / speed))
        .with_repeat(PaintRepeat::Forever)
        .with_curve(curves::linear);
    // Pre-start clamps to 0 (no negative elapsed).
    assert_eq!(a.sample(START - Duration::from_secs(1)).rotation, 0.0);
    // 0.25 s in → 1.0 rad, alpha untouched.
    let m = a.sample(START + Duration::from_millis(250));
    assert_eq!(m.rotation, 1.0, "rotation {}", m.rotation);
    assert_eq!(m.alpha, 1.0);
    // 2 s in → 8.0 rad, wrapped into [0, TAU): 8 - TAU ≈ 1.7168.
    let wrapped = a.sample(START + Duration::from_secs(2)).rotation;
    let expect = 8.0_f32.rem_euclid(TAU);
    assert_close(
        wrapped,
        expect,
        1e-6,
        "the sampler reaches 8 rad through the period's f32 seconds, an ulp \
         off the literal",
    );
    assert!((0.0..TAU).contains(&wrapped));
}

#[test]
fn spin_wakes_every_frame() {
    // `next_wake(prev)` must be <= now for any prev <= now, so `extend_predamaged`
    // repaints the spun rect each frame.
    let a = spinning(0, 1.0).anim;
    let prev = START + Duration::from_secs(3);
    let now = prev + Duration::from_millis(16);
    assert!(a.next_wake(prev).is_some_and(|wake| wake <= now));
}

/// The idle stop must hold at *sample* time: frames carrying a settled blink
/// past its cutoff are paint-only and run no record pass.
#[test]
fn blink_settles_solid_after_stop_and_stops_waking() {
    // Stop at 4 half-periods: boundaries at +1..+4 HP, then solid.
    let stop = HP * 4;
    let a = blink().with_repeat(PaintRepeat::Settle(stop));

    // Before the stop the phase alternates; odd multiples of HP are hidden.
    assert_eq!(a.sample(START + HP).alpha, 0.0);
    assert_eq!(a.sample(START + HP * 2).alpha, 1.0);
    assert_eq!(a.sample(START + HP * 3).alpha, 0.0);

    // At the stop and after: solid whatever the parity. `START + HP*5` is odd and
    // would be hidden without the stop.
    assert_eq!(a.sample(START + stop).alpha, 1.0);
    assert_eq!(a.sample(START + HP * 5).alpha, 1.0);
    assert_eq!(a.sample(START + Duration::from_secs(600)).alpha, 1.0);

    // Wakes run through the boundary reaching the stop (that transition must be
    // painted), then cease so an idle editor stops asking for frames.
    assert_eq!(a.next_wake(START + HP * 2), Some(START + HP * 3));
    assert_eq!(a.next_wake(START + HP * 3), Some(START + stop));
    assert_eq!(a.next_wake(START + stop), None);
    assert_eq!(a.next_wake(START + Duration::from_secs(600)), None);

    // A stop between boundaries still gets its own wake, since the settle is the
    // flip to paint. At 3.5 HP the phase is hidden (n = 3), so boundary-only wakes
    // would strand the caret invisible.
    let ragged = HP * 3 + HP / 2;
    let b = blink().with_repeat(PaintRepeat::Settle(ragged));
    assert_eq!(b.sample(START + HP * 3).alpha, 0.0);
    assert_eq!(b.sample(START + ragged).alpha, 1.0);
    assert_eq!(b.next_wake(START + HP * 3), Some(START + ragged));
    assert_eq!(b.next_wake(START + ragged), None);
}

/// A caller's own curve driving both channels off one pass.
///
/// Hand-computed with `alpha(0.2, 1.0)`, `turn(0.0, 0.5)`, one-second period,
/// `curve = |t| t * t`:
///
/// - at 0.5 s the curve gives 0.25: alpha `0.2 + 0.8 * 0.25 = 0.4`, turn
///   `0.5 * 0.25 = 0.125` turns = `TAU / 8` radians.
/// - at 1.0 s `Once` holds the end: alpha 1.0, turn a half = `TAU / 2`.
///
/// Samples are coerced where read: alpha clamps at either end, a NaN alpha end
/// or curve reads as no opacity, a non-finite turn as no turn. Halfway along a
/// linear curve `-1 -> 3` is `1` and `0 -> 4` is `2`; past `1` it clamps.
#[test]
fn samples_are_coerced_where_they_are_read() {
    let half = START + Duration::from_millis(500);
    let linear = |alpha: (f32, f32), turn: (f32, f32), curve: PaintCurve| {
        PaintAnimation::alpha(alpha.0, alpha.1)
            .with_turn(turn.0, turn.1)
            .with_started_at(START)
            .with_period(Duration::from_secs(1))
            .with_curve(curve)
            .sample(half)
    };
    let fine = linear((-1.0, 3.0), (0.0, 0.5), curves::linear);
    assert_eq!((fine.alpha, fine.rotation), (1.0, 0.25 * TAU));
    let nan_end = linear((f32::NAN, 1.0), (f32::NAN, 1.0), curves::linear);
    assert_eq!((nan_end.alpha, nan_end.rotation), (0.0, 0.0));
    let nan_curve = linear((0.0, 1.0), (0.0, 1.0), |_| f32::NAN);
    assert_eq!((nan_curve.alpha, nan_curve.rotation), (0.0, 0.0));
    let huge = linear((0.0, 1.0), (0.0, f32::MAX), curves::linear);
    assert_eq!(
        huge.rotation, 0.0,
        "a turn whose radians overflow is no turn"
    );
}

/// A fractional alpha is the point: the two shipped animations only answered 0
/// or 1.
#[test]
fn a_custom_curve_drives_both_channels_and_holds_at_the_end() {
    fn squared(t: f32) -> f32 {
        t * t
    }

    let a = PaintAnimation::alpha(0.2, 1.0)
        .with_turn(0.0, 0.5)
        .with_started_at(START)
        .with_period(Duration::from_secs(1))
        .with_curve(squared);

    let mid = a.sample(START + Duration::from_millis(500));
    assert_eq!(mid.alpha, 0.4, "alpha {}", mid.alpha);
    assert_eq!(mid.rotation, TAU / 8.0, "rotation {}", mid.rotation);

    let end = a.sample(START + Duration::from_secs(1));
    assert_eq!(end.alpha, 1.0, "alpha {}", end.alpha);
    assert_eq!(end.rotation, TAU / 2.0);
    assert_eq!(a.next_wake(START + Duration::from_secs(1)), None);

    // A turn of any range makes the damage bound the swept square; the cascade
    // asks without a `now` or a curve call.
    assert!(a.rotates());
    assert!(!PaintAnimation::alpha(0.0, 1.0).rotates());
}

/// `Settle` stops modifying the shape rather than holding an end value, so a
/// settled blink is a solid caret and a settled fade is the shape as recorded.
#[test]
fn a_settled_animation_stops_modifying_the_shape() {
    let a = PaintAnimation::alpha(0.0, 0.25)
        .with_started_at(START)
        .with_period(Duration::from_millis(100))
        .with_repeat(PaintRepeat::Settle(Duration::from_millis(250)));

    assert!(a.sample(START + Duration::from_millis(200)).alpha < 0.25);
    let settled = a.sample(START + Duration::from_millis(250));
    assert_eq!(settled.alpha, 1.0, "a settled animation multiplies by one");
    assert_eq!(settled.rotation, 0.0);
    assert_eq!(a.next_wake(START + Duration::from_millis(250)), None);
}

/// Zero steps would read as a never-animating shape.
#[test]
#[should_panic = "a count must be at least 1"]
fn zero_steps_is_a_caller_bug() {
    let _ = PaintAnimation::alpha(0.0, 1.0).with_steps(0);
}

/// Every part of an animation reaches its hash: the range either channel
/// drives, each timing field, and the curve. The same animation hashes the same.
#[test]
fn hash_static_covers_channel_timing_and_curve() {
    let hash = |anim: PaintAnimation| {
        let mut h = Hasher::new();
        anim.hash_static(&mut h);
        h.finish()
    };
    let base = blink();
    assert_eq!(hash(base), hash(blink()), "the same animation");
    for (label, other) in [
        (
            "alpha range",
            PaintAnimation {
                channel: PaintChannel {
                    alpha: Some((0.0, 0.5)),
                    turn: None,
                },
                ..base
            },
        ),
        (
            "the same range on the other channel",
            PaintAnimation {
                channel: PaintChannel {
                    alpha: None,
                    turn: Some((0.0, 1.0)),
                },
                ..base
            },
        ),
        ("started_at", base.with_started_at(START + HP)),
        ("period", base.with_period(HP)),
        ("repeat", base.with_repeat(PaintRepeat::Forever)),
        ("steps", base.with_steps(3)),
        ("curve", base.with_curve(curves::linear)),
    ] {
        assert_ne!(hash(base), hash(other), "{label}");
    }
}
