use super::*;

/// Keys alone, in fire order; serials have their own cases.
fn fired(wheel: &mut ExpiryWheel<u32>, frame: u64) -> Vec<u32> {
    fired_with_serials(wheel, frame)
        .into_iter()
        .map(|(key, _)| key)
        .collect()
}

fn fired_with_serials(wheel: &mut ExpiryWheel<u32>, frame: u64) -> Vec<(u32, TicketSeq)> {
    let mut out = Vec::new();
    wheel.retire(frame, |key, seq| {
        out.push((key, seq));
        None
    });
    out
}

/// The wheel is a schedule, not a policy: a ticket comes back once, on its due frame.
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

/// Each filing gets its own serial, returned by `schedule`.
#[test]
fn a_ticket_comes_back_under_the_serial_it_was_filed_with() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    let first = wheel.schedule(1, 2);
    let second = wheel.schedule(2, 3);
    assert_ne!(first, second, "every filing gets its own serial");

    let mut seen = fired_with_serials(&mut wheel, 3);
    seen.sort_unstable();
    assert_eq!(seen, vec![(1, first), (2, second)]);
}

/// A re-file keeps the serial it fired under, so an owner stamps only at its own `schedule`.
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

/// A clock advancing by more than one must not step over buckets.
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

/// A jump wider than the ring aliases every bucket, so everything is handed back, even not-yet-due tickets (callers re-file); each keeps its own serial.
#[test]
fn a_jump_wider_than_the_ring_hands_back_everything() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    let near = wheel.schedule(1, 2);
    let far = wheel.schedule(2, 9);

    let mut swept = fired_with_serials(&mut wheel, 100);
    swept.sort_unstable();
    assert_eq!(
        swept,
        vec![(1, near), (2, far)],
        "both, though only one was due, under their true serials",
    );

    assert!(fired(&mut wheel, 200).is_empty());
}

/// The owner re-file pattern: fire early, find the entry live, put it back.
#[test]
fn refiling_from_inside_a_drain_defers_without_extra_tickets() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    wheel.schedule(7, 2);

    // The entry is touched every frame but files nothing until its ticket fires at 2; that ticket covers the span to 6.
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

    assert_eq!(fired(&mut wheel, 6), vec![7]);
    assert!(
        fired(&mut wheel, 20).is_empty(),
        "a ticket not re-filed does not come back",
    );
}

/// A ticket filed further out than the ring fires early instead of aliasing a drained bucket; the clamp moves the frame, not the serial.
#[test]
fn a_ticket_past_the_ring_fires_early_rather_than_late() {
    // Horizon 8 rounds to 16 slots, so 15 frames is the furthest safe bucket; 200 would alias frame 8.
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    let clamped = wheel.schedule(1, 200);

    let mut fired_at = None;
    for frame in 1..=15 {
        let fired = fired_with_serials(&mut wheel, frame);
        if !fired.is_empty() {
            assert_eq!(fired, vec![(1, clamped)], "under its own serial");
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

/// Horizon rounds up to a power of two.
#[test]
fn horizon_rounds_up_and_indexes_in_range() {
    for (horizon, slots) in [(1u64, 2usize), (3, 4), (8, 16), (120, 128), (121, 128)] {
        let wheel = ExpiryWheel::<u32>::with_horizon(horizon);
        assert_eq!(wheel.buckets.len(), slots, "horizon {horizon}");
        assert_eq!(wheel.mask, slots as u64 - 1, "horizon {horizon}");
        assert_eq!(
            ExpiryWheel::<u32>::slots_for_horizon(horizon),
            slots as u64,
            "horizon {horizon}",
        );
        assert!(
            horizon <= wheel.mask,
            "horizon {horizon} must fit the schedule assert",
        );
    }
    // The shaped-buffer cache's keep: 120 + 15 spread = 135, + 2 = 137, one spare → 138, rounded up → 256.
    assert_eq!(ExpiryWheel::<u32>::with_keep(135).buckets.len(), 256);
    assert_eq!(ExpiryWheel::<u32>::slots_for_keep(135), 256);
}

/// Buckets draw chunks of 16 from one pool: a burst of 4000 tickets in one bucket takes
/// 4000 / 16 = 250 chunks and no more, and once drained, a burst of 4000 in another bucket takes
/// the same 250 back, so the pool grows only past its peak. A 17th ticket opens a second chunk.
#[test]
fn buckets_share_one_pool_that_grows_only_past_its_peak() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    assert_eq!(wheel.chunks(), 0, "an empty wheel holds no chunk");

    for key in 0..16 {
        wheel.schedule(key, 3);
    }
    assert_eq!(wheel.chunks(), 1, "sixteen fill one chunk");
    wheel.schedule(16, 3);
    assert_eq!(wheel.chunks(), 2, "the seventeenth opens a second");
    wheel.schedule(17, 4);
    assert_eq!(wheel.chunks(), 3, "another bucket takes a chunk of its own");
    let mut drained = fired(&mut wheel, 4);
    drained.sort_unstable();
    assert_eq!(drained, (0..18).collect::<Vec<_>>());
    assert_eq!(wheel.pending(), 0);

    for key in 0..4000 {
        wheel.schedule(key, 6);
    }
    assert_eq!(wheel.chunks(), 250, "the three free ones and 247 new");
    assert_eq!(fired(&mut wheel, 6).len(), 4000);
    let slots = wheel.tickets.capacity();
    let free = wheel.free.capacity();
    for key in 0..4000 {
        wheel.schedule(key, 9);
    }
    assert_eq!(
        wheel.chunks(),
        250,
        "a burst in another bucket reuses the pool"
    );
    assert_eq!(
        wheel.tickets.capacity(),
        slots,
        "and allocates no ticket slot"
    );
    assert_eq!(
        fired(&mut wheel, 9),
        (0..4000).collect::<Vec<_>>(),
        "in filed order"
    );
    assert_eq!(
        wheel.free.capacity(),
        free,
        "handing every chunk back allocates nothing"
    );
    assert_eq!(wheel.free.len(), 250);
}

/// The pool starts with room for a chunk per bucket: a horizon of 7 is 8 buckets, so 8 chunks of 16
/// tickets fit. One ticket on each of frames 1 to 7 takes 7 chunks. Frame 1's 17th ticket opens the
/// 8th, and its 32nd fills it, so the room is full with no buffer grown. Its 33rd opens a 9th.
#[test]
fn the_pool_grows_no_buffer_within_a_chunk_per_bucket() {
    fn capacities(wheel: &ExpiryWheel<u32>) -> [usize; 3] {
        [
            wheel.tickets.capacity(),
            wheel.links.capacity(),
            wheel.free.capacity(),
        ]
    }
    let mut wheel = ExpiryWheel::<u32>::with_horizon(7);
    let room = [8 * 16, 8, 8];
    assert_eq!(capacities(&wheel), room);

    for due in 1..=7 {
        wheel.schedule(0, due);
    }
    assert_eq!(wheel.chunks(), 7);

    for _ in 2..=16 {
        wheel.schedule(0, 1);
    }
    assert_eq!(wheel.chunks(), 7, "16 tickets fit frame 1's chunk");

    wheel.schedule(0, 1);
    assert_eq!(wheel.chunks(), 8, "the 17th opens the 8th chunk");

    for _ in 18..=32 {
        wheel.schedule(0, 1);
    }
    assert_eq!(wheel.chunks(), 8, "the 32nd fills it");
    assert_eq!(capacities(&wheel), room, "the room holds all 8");

    wheel.schedule(0, 1);
    assert_eq!(wheel.chunks(), 9, "the 33rd opens a 9th");
    assert!(
        capacities(&wheel)[1] > room[1],
        "a 9th chunk grows the links"
    );
}

/// A clear drops every ticket and keeps every chunk in the pool.
#[test]
fn a_clear_keeps_the_pool() {
    let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
    for key in 0..40 {
        wheel.schedule(key, 2 + u64::from(key % 3));
    }
    assert_eq!(
        wheel.chunks(),
        3,
        "40 over three buckets: 14, 13 and 13, a chunk each"
    );
    wheel.clear();
    assert_eq!(wheel.pending(), 0);
    assert!(fired(&mut wheel, 20).is_empty());
    for key in 0..40 {
        wheel.schedule(key, 21);
    }
    assert_eq!(
        wheel.chunks(),
        3,
        "40 in one bucket fit the three pooled chunks"
    );
}
