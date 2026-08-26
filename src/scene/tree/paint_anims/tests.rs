use crate::scene::tree::paint_anims::*;

const HP: Duration = Duration::from_millis(500);
const START: Duration = Duration::from_secs(1);
/// Long enough that the tests below never reach it, so each one
/// isolates phase behaviour from the settle.
const NO_STOP: Duration = Duration::MAX;

/// A blink that runs forever, for the cases about phase alone.
fn blink() -> PaintAnim {
    PaintAnim::BlinkOpacity {
        half_period: HP,
        started_at: START,
        stop_after: NO_STOP,
    }
}

fn spinning(speed: f32) -> PaintAnimEntry {
    PaintAnimEntry {
        anim: PaintAnim::Spin {
            speed,
            started_at: START,
        },
        row: 0,
        node_idx: 0,
    }
}

#[test]
fn sparse_cursor_samples_boundaries_and_advances_across_skipped_animations() {
    const LAST_SHAPE: u32 = 1_000_000;
    let mut anims = PaintAnims::default();
    anims.push_entry(0, spinning(1.0));
    anims.push_entry(5, spinning(2.0));
    anims.push_entry(10, spinning(3.0));
    anims.push_entry(LAST_SHAPE, spinning(4.0));

    assert_eq!(anims.shape_indices, [0, 5, 10, LAST_SHAPE]);
    assert_eq!(anims.entries.len(), 4);
    assert_eq!(
        std::mem::size_of_val(anims.shape_indices.as_slice()),
        4 * std::mem::size_of::<u32>(),
    );

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

    // A jump that lands *between* two registrations: culling skipped
    // shape 5, and shape 6 sits below the next registered index. It
    // owns no animation, and taking 10's would both misparent it and
    // leave shape 10 unanimated.
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

    let shape_capacity = anims.shape_indices.capacity();
    let entry_capacity = anims.entries.capacity();
    anims.clear();
    assert!(anims.shape_indices.is_empty());
    assert!(anims.entries.is_empty());
    assert_eq!(anims.shape_indices.capacity(), shape_capacity);
    assert_eq!(anims.entries.capacity(), entry_capacity);
}

/// The reading half of the ordering contract `push_entry` asserts on
/// the recording half. Debug-only, because the check needs a field
/// the release cursor doesn't carry.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "must be monotonic")]
fn sampling_backwards_is_a_caller_bug() {
    let mut anims = PaintAnims::default();
    anims.push_entry(2, spinning(1.0));
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
    // On the boundary: still wake at the *next* one (strictly
    // future).
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

#[test]
fn zero_period_never_wakes() {
    let a = PaintAnim::BlinkOpacity {
        half_period: Duration::ZERO,
        started_at: START,
        stop_after: NO_STOP,
    };
    // Degenerate, but must not panic. `next_wake` returns `None` so
    // the wake folder drops it.
    assert_eq!(a.next_wake(START + Duration::from_secs(1)), None);
}

#[test]
fn spin_angle_is_elapsed_times_speed_wrapped() {
    let speed = 4.0; // rad/s
    let a = PaintAnim::Spin {
        speed,
        started_at: START,
    };
    // Pre-start clamps to 0 (no negative elapsed).
    assert_eq!(a.sample(START - Duration::from_secs(1)).rotation, 0.0);
    // 0.25 s in → 1.0 rad, alpha untouched.
    let m = a.sample(START + Duration::from_millis(250));
    assert!((m.rotation - 1.0).abs() < 1e-5, "rotation {}", m.rotation);
    assert_eq!(m.alpha, 1.0);
    // 2 s in → 8.0 rad, wrapped into [0, TAU): 8 - TAU ≈ 1.7168.
    let wrapped = a.sample(START + Duration::from_secs(2)).rotation;
    let expect = 8.0_f32.rem_euclid(TAU);
    assert!((wrapped - expect).abs() < 1e-4, "wrapped {wrapped}");
    assert!((0.0..TAU).contains(&wrapped));
}

#[test]
fn spin_wakes_every_frame() {
    // `next_wake(prev)` must be <= now for any prev <= now so
    // `extend_predamaged` repaints the spun rect each frame.
    let a = PaintAnim::Spin {
        speed: 1.0,
        started_at: START,
    };
    let prev = START + Duration::from_secs(3);
    let now = prev + Duration::from_millis(16);
    assert!(a.next_wake(prev).is_some_and(|wake| wake <= now));
}

/// The idle stop has to hold at *sample* time, because the frames
/// that carry a settled blink past its cutoff are paint-only — no
/// record pass runs on them to re-decide anything.
#[test]
fn blink_settles_solid_after_stop_and_stops_waking() {
    // Stop at 4 half-periods: boundaries at +1..+4 HP, then solid.
    let stop = HP * 4;
    let a = PaintAnim::BlinkOpacity {
        half_period: HP,
        started_at: START,
        stop_after: stop,
    };

    // Before the stop the phase still alternates: odd multiples of
    // HP are the hidden ones.
    assert_eq!(a.sample(START + HP).alpha, 0.0);
    assert_eq!(a.sample(START + HP * 2).alpha, 1.0);
    assert_eq!(a.sample(START + HP * 3).alpha, 0.0);

    // At the stop and ever after: solid, whatever the parity says.
    // `START + HP*5` is an odd multiple — it would be hidden if the
    // stop weren't applied.
    assert_eq!(a.sample(START + stop).alpha, 1.0);
    assert_eq!(a.sample(START + HP * 5).alpha, 1.0);
    assert_eq!(a.sample(START + Duration::from_secs(600)).alpha, 1.0);

    // Wakes run up to and including the boundary that reaches the
    // stop — that transition still has to be painted — and cease
    // afterwards, so an idle editor stops asking for frames.
    assert_eq!(a.next_wake(START + HP * 2), Some(START + HP * 3));
    assert_eq!(a.next_wake(START + HP * 3), Some(START + stop));
    assert_eq!(a.next_wake(START + stop), None);
    assert_eq!(a.next_wake(START + Duration::from_secs(600)), None);

    // A stop that lands *between* boundaries still gets its own
    // wake, since the settle is the flip that has to be painted.
    // 3.5 half-periods in, the phase is the hidden one (n = 3), so
    // waking only on boundaries would strand the caret invisible.
    let ragged = HP * 3 + HP / 2;
    let b = PaintAnim::BlinkOpacity {
        half_period: HP,
        started_at: START,
        stop_after: ragged,
    };
    assert_eq!(b.sample(START + HP * 3).alpha, 0.0);
    assert_eq!(b.sample(START + ragged).alpha, 1.0);
    assert_eq!(b.next_wake(START + HP * 3), Some(START + ragged));
    assert_eq!(b.next_wake(START + ragged), None);
}
