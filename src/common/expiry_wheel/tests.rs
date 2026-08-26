use super::*;

/// Keys alone, in fire order: most tests here pin *which* tickets
/// fire, and serials have their own cases below.
fn fired(wheel: &mut ExpiryWheel<u32>, frame: u64) -> Vec<u32> {
    let mut out = Vec::new();
    wheel.retire(frame, |key, _| {
        out.push(key);
        None
    });
    out
}

/// The wheel is a schedule, not a policy: a ticket comes back on its
/// due frame, once, and only then.
#[test]
fn tickets_fire_on_their_due_frame_and_only_then() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);

    wheel.schedule(10, 3);
    wheel.schedule(20, 5);
    wheel.schedule(21, 5);

    for frame in 1..=2 {
        assert!(
            fired(&mut wheel, frame).is_empty(),
            "nothing due at {frame}"
        );
    }
    assert_eq!(fired(&mut wheel, 3), vec![10]);
    assert!(
        fired(&mut wheel, 4).is_empty(),
        "a fired ticket must not fire twice",
    );
    assert_eq!(
        fired(&mut wheel, 5),
        vec![20, 21],
        "one bucket can hold several keys",
    );
    assert!(fired(&mut wheel, 6).is_empty());
}

/// Every filing gets its own serial, and a ticket comes back under
/// the one its `schedule` returned — the stamp an owner matches
/// against to tell its live ticket from a supplanted one.
#[test]
fn a_ticket_comes_back_under_the_serial_it_was_filed_with() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    let first = wheel.schedule(1, 2);
    let second = wheel.schedule(2, 3);
    assert_ne!(first, second, "every filing gets its own serial");

    let mut seen = Vec::new();
    wheel.retire(3, |key, seq| {
        seen.push((key, seq));
        None
    });
    seen.sort_unstable();
    assert_eq!(seen, vec![(1, first), (2, second)]);
}

/// A re-file keeps the serial it fired under, so an owner stamps only
/// where it decides something — at its own `schedule` — and never for
/// a ticket the wheel put back on its behalf.
#[test]
fn a_refile_keeps_its_serial() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    let seq = wheel.schedule(1, 2);

    let mut seen = Vec::new();
    wheel.retire(2, |key, s| {
        seen.push((key, s));
        Some(5)
    });
    wheel.retire(5, |key, s| {
        seen.push((key, s));
        None
    });
    assert_eq!(seen, vec![(1, seq), (1, seq)], "one serial, two firings");
}

/// The two mechanisms that make the *frame* an unusable identity, and
/// which a serial is immune to: a ticket the clamp moved, and a drain
/// that aliased every bucket. Either would let an entry stop recognising
/// its own live ticket.
#[test]
fn clamped_and_aliased_tickets_keep_true_serials() {
    // Horizon 8 rounds to 16 slots, so 200 is far past the ring.
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    let clamped = wheel.schedule(1, 200);
    let mut seen = None;
    for frame in 1..=15 {
        wheel.retire(frame, |key, seq| {
            seen = Some((key, seq));
            None
        });
        if seen.is_some() {
            break;
        }
    }
    assert_eq!(
        seen,
        Some((1, clamped)),
        "a clamped ticket keeps its serial"
    );

    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    let near = wheel.schedule(1, 2);
    let far = wheel.schedule(2, 9);
    let mut seen = Vec::new();
    wheel.retire(100, |key, seq| {
        seen.push((key, seq));
        None
    });
    seen.sort_unstable();
    assert_eq!(
        seen,
        vec![(1, near), (2, far)],
        "an aliased drain reports true serials, not its buckets' frames",
    );
}

/// A clock that advances by more than one — two windows recording
/// before one shared submit — must not step over the buckets in
/// between.
#[test]
fn a_jumping_clock_drains_every_bucket_it_passed() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    for (key, due) in [(1u32, 2u64), (2, 3), (3, 4), (4, 7)] {
        wheel.schedule(key, due);
    }

    let mut swept = fired(&mut wheel, 4);
    swept.sort_unstable();
    assert_eq!(swept, vec![1, 2, 3], "frames 1..=4 all drained");

    assert_eq!(
        fired(&mut wheel, 9),
        vec![4],
        "the later ticket still fires"
    );
}

/// A jump wider than the ring aliases every bucket, so everything is
/// handed back — including tickets that were not really due. Callers
/// re-file those, so the contract is "never a missed ticket", not
/// "never an early one".
#[test]
fn a_jump_wider_than_the_ring_hands_back_everything() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    // Horizon 8 rounds to 16 slots.
    wheel.schedule(1, 2);
    wheel.schedule(2, 9);

    let mut swept = fired(&mut wheel, 100);
    swept.sort_unstable();
    assert_eq!(swept, vec![1, 2], "both, though only one was due");

    // And the ring is genuinely empty afterwards — an early-drained
    // ticket must not also be left behind to fire again.
    assert!(fired(&mut wheel, 200).is_empty());
}

/// The re-file pattern the owners run: fire early, find the entry
/// still live, put it back. This is what lets an entry touched every
/// frame file one ticket per horizon rather than one per frame.
#[test]
fn refiling_from_inside_a_drain_defers_without_extra_tickets() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    wheel.schedule(7, 2);

    // The owner "touches" the entry every frame, pushing its real
    // deadline out — but files nothing until its ticket fires at 2,
    // and then that one ticket covers the whole span to 6.
    let mut fired_at = Vec::new();
    for frame in 1..=5 {
        wheel.retire(frame, |_, _| {
            fired_at.push(frame);
            Some(6)
        });
    }
    assert_eq!(
        fired_at,
        vec![2],
        "one ticket per deferral, not one per frame",
    );
    assert_eq!(wheel.pending(), 1, "and no duplicate left behind");

    // Let it lapse: it fires at 6, is not re-filed, and is gone.
    assert_eq!(fired(&mut wheel, 6), vec![7]);
    assert!(
        fired(&mut wheel, 20).is_empty(),
        "a ticket not re-filed does not come back",
    );
}

/// A ticket filed further out than the ring is wide must fire
/// *early*, not alias its way into a bucket already drained and fire
/// a whole ring late. The owner re-files it, so the only cost is one
/// extra visit.
#[test]
fn a_ticket_past_the_ring_fires_early_rather_than_late() {
    // Horizon 8 rounds to 16 slots, so the furthest safe bucket is
    // 15 frames out; 200 would alias frame 8 (200 % 16 == 8).
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    wheel.schedule(1, 200);

    // Walk the frames a naive `due & mask` would have fired on, plus
    // the whole first ring, and pin that it came back inside it.
    let mut fired_at = None;
    for frame in 1..=15 {
        if !fired(&mut wheel, frame).is_empty() {
            fired_at = Some(frame);
            break;
        }
    }
    assert_eq!(fired_at, Some(15), "clamped to the ring's far edge");
}

#[test]
fn clear_drops_every_outstanding_ticket() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    wheel.schedule(1, 2);
    wheel.schedule(2, 6);

    wheel.clear();
    assert_eq!(wheel.pending(), 0);
    assert!(fired(&mut wheel, 9).is_empty());
}

/// Horizon rounds up to a power of two, and the mask must index
/// inside the ring for every frame a caller can reach.
#[test]
fn horizon_rounds_up_and_indexes_in_range() {
    for (horizon, slots) in [(1u64, 2usize), (3, 4), (8, 16), (120, 128), (121, 128)] {
        let wheel = ExpiryWheel::<u32>::with_horizon(horizon);
        assert_eq!(wheel.buckets.len(), slots, "horizon {horizon}");
        assert_eq!(wheel.mask, slots as u64 - 1, "horizon {horizon}");
        assert!(
            horizon <= wheel.mask,
            "horizon {horizon} must fit the schedule assert",
        );
    }
}
