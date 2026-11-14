use crate::common::hash::Hasher;
use crate::shape::paint_animation::*;
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
        .with_steps(NonZeroU32::new(2).unwrap())
        .with_repeat(PaintRepeat::Settle(NO_STOP))
        .with_curve(PaintCurve::SQUARE)
}

/// A turn at `speed` radians per second that never stops.
fn spinning(speed: f32) -> PaintAnimation {
    PaintAnimation::turn(0.0, 1.0)
        .with_started_at(START)
        .with_period(Duration::from_secs_f32(TAU / speed))
        .with_repeat(PaintRepeat::Forever)
        .with_curve(PaintCurve::LINEAR)
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

    // Past 2^32 boundaries the wake is still the next one, never one long gone. A 2 ns
    // period in two steps has a boundary every 1 ns, so 5 s in is boundary 5e9, past
    // `u32::MAX` (4 294 967 295); at 4 ns a step is 2 ns, and 1 ns into one leaves 1 ns.
    let fine = |period| {
        PaintAnimation::alpha(0.0, 1.0)
            .with_started_at(START)
            .with_period(Duration::from_nanos(period))
            .with_steps(NonZeroU32::new(2).unwrap())
            .with_repeat(PaintRepeat::Forever)
    };
    let late = START + Duration::from_secs(5);
    assert_eq!(
        fine(2).next_wake(late),
        Some(late + Duration::from_nanos(1))
    );
    let mid_step = late + Duration::from_nanos(1);
    assert_eq!(
        fine(4).next_wake(mid_step),
        Some(late + Duration::from_nanos(2))
    );
}

/// Each wake lands on the first nanosecond of the next step: the sample there shows step `k mod n`
/// after `k` wakes, and 1 ns before it still shows the step before. Covers every `n` below 200 at a
/// 1 s period, where an f32 phase put 810 boundaries one step late, and periods `n` does not divide,
/// down to steps of 1 ns.
#[test]
fn every_wake_samples_the_next_step() {
    let one_ns = Duration::from_nanos(1);
    let cases = (1..200).map(|n| (n, Duration::from_secs(1))).chain([
        (22, Duration::from_nanos(1_000_003)),
        (41, Duration::from_nanos(999_999_937)),
        (3, Duration::from_nanos(10)),
        (7, Duration::from_nanos(7)),
    ]);
    for (n, period) in cases {
        let timing = PaintAnimation::alpha(0.0, 1.0)
            .with_started_at(START)
            .with_period(period)
            .with_steps(NonZeroU32::new(n).unwrap())
            .with_repeat(PaintRepeat::Forever)
            .timing;
        let step = |k: u32| f64::from(k % n) / f64::from(n);
        let mut now = START;
        assert_eq!(timing.phase(now), Some(step(0)));
        for k in 1..=3 * n {
            now = timing.next_wake(now).unwrap();
            assert_eq!(
                timing.phase(now),
                Some(step(k)),
                "n {n}, {period:?}, wake {k}"
            );
            assert_eq!(
                timing.phase(now - one_ns),
                Some(step(k - 1)),
                "n {n}, {period:?}, before wake {k}",
            );
        }
        assert_eq!(now, START + period * 3, "n {n}, {period:?}");
    }

    // Under 1 ns a step, a wake skips the steps no nanosecond starts. Five steps of a 3 ns pass
    // start at ⌈3k/5⌉ ns = 0, 1, 2, 2, 3 for k = 0..4: the wakes at 1 and 2 ns show steps 1 and 3,
    // and the one at 3 ns opens the next pass.
    let timing = PaintAnimation::alpha(0.0, 1.0)
        .with_started_at(START)
        .with_period(Duration::from_nanos(3))
        .with_steps(NonZeroU32::new(5).unwrap())
        .with_repeat(PaintRepeat::Forever)
        .timing;
    let mut now = START;
    for (ns, phase) in [(1, 0.2), (2, 0.6), (3, 0.0)] {
        now = timing.next_wake(now).unwrap();
        assert_eq!(now, START + Duration::from_nanos(ns));
        assert_eq!(timing.phase(now), Some(phase), "{ns} ns");
    }
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
            .with_curve(PaintCurve::LINEAR);
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
        .with_curve(PaintCurve::LINEAR);
    // Pre-start clamps to 0 (no negative elapsed).
    assert_eq!(a.sample(START - Duration::from_secs(1)).rotation, 0.0);
    // 0.25 s in → 1.0 rad, alpha untouched.
    let m = a.sample(START + Duration::from_millis(250));
    assert_eq!(m.rotation, 1.0, "rotation {}", m.rotation);
    assert_eq!(m.alpha, 1.0);
    // 2 s in → 8.0 rad, wrapped into [0, TAU): 8 - TAU ≈ 1.7168.
    let wrapped = a.sample(START + Duration::from_secs(2)).rotation;
    let expect = 8.0_f32.rem_euclid(TAU);
    domain::internals::assert_close(
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
    let a = spinning(1.0);
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

/// Samples are coerced where read: alpha clamps at either end, a NaN alpha end or curve reads as
/// no opacity, a non-finite turn as no turn. Halfway along a linear curve `-1 -> 3` is `1` and
/// `0 -> 4` is `2`; past `1` it clamps.
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
    let fine = linear((-1.0, 3.0), (0.0, 0.5), PaintCurve::LINEAR);
    assert_eq!((fine.alpha, fine.rotation), (1.0, 0.25 * TAU));
    let nan_end = linear((f32::NAN, 1.0), (f32::NAN, 1.0), PaintCurve::LINEAR);
    assert_eq!((nan_end.alpha, nan_end.rotation), (0.0, 0.0));
    let nan_curve = linear((0.0, 1.0), (0.0, 1.0), PaintCurve::new(|_| f32::NAN));
    assert_eq!((nan_curve.alpha, nan_curve.rotation), (0.0, 0.0));
    let huge = linear((0.0, 1.0), (0.0, f32::MAX), PaintCurve::LINEAR);
    assert_eq!(
        huge.rotation, 0.0,
        "a turn whose radians overflow is no turn"
    );
}

/// A caller's own curve driving both channels off one pass, hand-computed with `alpha(0.2, 1.0)`,
/// `turn(0.0, 0.5)`, one-second period, `curve = |t| t * t`:
///
/// - at 0.5 s the curve gives 0.25: alpha `0.2 + 0.8 * 0.25 = 0.4`, turn
///   `0.5 * 0.25 = 0.125` turns = `TAU / 8` radians.
/// - at 1.0 s `Once` holds the end: alpha 1.0, turn a half = `TAU / 2`.
#[test]
fn a_custom_curve_drives_both_channels_and_holds_at_the_end() {
    fn squared(t: f32) -> f32 {
        t * t
    }

    let a = PaintAnimation::alpha(0.2, 1.0)
        .with_turn(0.0, 0.5)
        .with_started_at(START)
        .with_period(Duration::from_secs(1))
        .with_curve(PaintCurve::new(squared));

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

    assert_eq!(
        a.sample(START + Duration::from_millis(200)).alpha,
        0.0,
        "two whole passes in, the third starts again from `from`",
    );
    let settled = a.sample(START + Duration::from_millis(250));
    assert_eq!(settled.alpha, 1.0, "a settled animation multiplies by one");
    assert_eq!(settled.rotation, 0.0);
    assert_eq!(a.next_wake(START + Duration::from_millis(250)), None);
}

/// Every part of an animation reaches its hash: the range either channel
/// drives, each timing field, and the curve. The same animation hashes the same.
#[test]
fn hash_static_covers_channel_timing_and_curve() {
    let hash = |animation: PaintAnimation| {
        let mut h = Hasher::new();
        animation.hash_static(&mut h);
        h.finish()
    };
    let base = blink();
    assert_eq!(hash(base), hash(blink()), "the same animation");
    for (label, other) in [
        (
            "alpha range",
            PaintAnimation {
                channels: PaintChannels {
                    alpha: Some(PaintRange { from: 0.0, to: 0.5 }),
                    turn: None,
                },
                ..base
            },
        ),
        (
            "the same range on the other channel",
            PaintAnimation {
                channels: PaintChannels {
                    alpha: None,
                    turn: Some(PaintRange { from: 0.0, to: 1.0 }),
                },
                ..base
            },
        ),
        ("started_at", base.with_started_at(START + HP)),
        ("period", base.with_period(HP)),
        ("repeat", base.with_repeat(PaintRepeat::Forever)),
        ("held", base.with_repeat(PaintRepeat::Held)),
        ("steps", base.with_steps(NonZeroU32::new(3).unwrap())),
        ("curve", base.with_curve(PaintCurve::LINEAR)),
    ] {
        assert_ne!(hash(base), hash(other), "{label}");
    }
}

/// Under reduced motion progress runs on unchanged, a decorative single pass paints its end value
/// from the first frame, and a decorative repeat, settling or not, goes.
#[test]
fn reduced_motion_keeps_progress_and_lands_decoration() {
    let spinner = spinning(1.0).with_motion(MotionRole::Essential);
    let kept = spinner.reduced().expect("progress runs");
    assert_eq!(
        (kept.channels, kept.timing),
        (spinner.channels, spinner.timing)
    );
    assert!(spinning(1.0).reduced().is_none(), "a decorative turn");
    assert!(blink().reduced().is_none(), "a settling blink");

    let fade = PaintAnimation::alpha(0.25, 1.0)
        .with_started_at(START)
        .with_period(HP);
    let landed = fade.reduced().expect("a single pass lands");
    assert_eq!(landed.timing.repeat, PaintRepeat::Held);
    assert_eq!(landed.sample(Duration::ZERO), fade.sample(START + HP));
    assert_eq!(landed.sample(Duration::ZERO).alpha, 1.0, "the fade's end");
    assert_eq!(
        landed.sample(Duration::ZERO).time,
        0.5,
        "an effect reads the end of the 500 ms pass"
    );
    assert_eq!(landed.next_wake(Duration::ZERO), None);
    let held_later = landed.with_started_at(START);
    assert_eq!(
        (
            held_later.sample(Duration::ZERO),
            held_later.next_wake(Duration::ZERO)
        ),
        (landed.sample(Duration::ZERO), None),
        "a held pass ignores its start: it neither waits for it nor wakes at it"
    );
    let again = landed.reduced().expect("a held pass stays");
    assert_eq!(again.timing, landed.timing);
    assert_ne!(
        fade.sample(START).alpha,
        1.0,
        "unreduced, it starts at 0.25"
    );
}

/// An effect reads the seconds into its pass of `2 s` from `START`, each counted from the exact
/// nanoseconds:
/// - a loop reads 0 before the start, 0.5 s at 0.5 s in, 0 at the wrap, 0.75 s at 2.75 s in;
/// - a single pass reads 0.5 s, then its whole 2 s from its end on, and stops waking;
/// - four steps read each step's start: 0.7 s in is step 1 of 4, 0.5 s, and so is 2.6 s in;
/// - a loop that settles at 3 s reads 0.5 s at 2.5 s in, then 0 as unanimated.
///
/// A clock drives no channel: alpha holds at 1 and rotation at 0.
#[test]
fn an_effect_reads_the_time_into_its_pass() {
    let ms = Duration::from_millis;
    let clock = |repeat| {
        PaintAnimation::clock()
            .with_started_at(START)
            .with_period(Duration::from_secs(2))
            .with_repeat(repeat)
    };
    let looping = clock(PaintRepeat::Forever);
    let once = clock(PaintRepeat::Once);
    let stepped = clock(PaintRepeat::Forever).with_steps(NonZeroU32::new(4).unwrap());
    let settling = clock(PaintRepeat::Settle(Duration::from_secs(3)));
    let cases = [
        (looping, START - ms(200), 0.0),
        (looping, START + ms(500), 0.5),
        (looping, START + ms(2000), 0.0),
        (looping, START + ms(2750), 0.75),
        (once, START + ms(500), 0.5),
        (once, START + ms(2000), 2.0),
        (once, START + ms(5000), 2.0),
        (stepped, START + ms(700), 0.5),
        (stepped, START + ms(2600), 0.5),
        (settling, START + ms(2500), 0.5),
        (settling, START + ms(3000), 0.0),
    ];
    for (i, (animation, now, time)) in cases.into_iter().enumerate() {
        assert_eq!(animation.sample(now).time, time, "case {i} at {now:?}");
    }
    assert_eq!(once.next_wake(START + ms(2000)), None);
    let sampled = looping.sample(START + ms(500));
    assert_eq!((sampled.alpha, sampled.rotation), (1.0, 0.0));
    assert!(!looping.drives_channel());
    assert!(PaintAnimation::alpha(0.0, 1.0).drives_channel());
    assert!(PaintAnimation::turn(0.0, 1.0).drives_channel());
}

/// A long run reads its pass as precisely as the first: 10⁹ s and 123 456 789 ns into a 1 s loop
/// reads 0.123 456 789 s as f32 rounds it. The seconds as one f64, 1 000 000 000.123 456 789,
/// hold only 7 digits past the point, so a phase taken from them lands ulps away.
#[test]
fn a_long_run_keeps_the_time_into_its_pass_exact() {
    let looping = PaintAnimation::clock()
        .with_period(Duration::from_secs(1))
        .with_repeat(PaintRepeat::Forever);
    let now = Duration::new(1_000_000_000, 123_456_789);
    // `as` rounds to the nearest f32, as the sample's own conversion does.
    let exact = 0.123_456_789_f64 as f32;
    assert_eq!(looping.sample(now).time, exact);
    let from_seconds = (now.as_secs_f64() / 1.0).fract() as f32;
    assert_ne!(from_seconds, exact, "the f64 seconds lose it");
}
