use super::*;

/// The atlas is generic over its key, so its own tests use the cheapest one
/// that satisfies the bounds rather than either tenant's — nothing here
/// depends on what a key means.
type TestKey = u16;

fn key(id: u16) -> TestKey {
    id
}

fn slot(alloc: Option<AllocId>, last_use: u64) -> AtlasSlot {
    AtlasSlot {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        left: 0,
        top: 0,
        content: ContentType::Mask,
        alloc,
        generation: 0,
        last_use,
    }
}

#[test]
fn packed_metadata_checks_every_wire_boundary() {
    let placement = |width, height, left, top| (width, height, left, top);
    let packed = |(width, height, left, top)| PackedMetadata::new(width, height, left, top);
    assert_eq!(
        packed(placement(0, 0, 0, 0)).unwrap(),
        PackedMetadata {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
        }
    );
    assert_eq!(
        packed(placement(
            u16::MAX as u32,
            u16::MAX as u32,
            i16::MIN as i32,
            i16::MAX as i32,
        ))
        .unwrap(),
        PackedMetadata {
            width: u16::MAX,
            height: u16::MAX,
            left: i16::MIN,
            top: i16::MAX,
        }
    );
    assert_eq!(
        packed(placement(1, 1, i16::MAX as i32, i16::MIN as i32)).unwrap(),
        PackedMetadata {
            width: 1,
            height: 1,
            left: i16::MAX,
            top: i16::MIN,
        }
    );

    let invalid = [
        (u16::MAX as u32 + 1, 1, 0, 0, "width above u16"),
        (1, u16::MAX as u32 + 1, 0, 0, "height above u16"),
        (1, 1, i16::MIN as i32 - 1, 0, "left below i16"),
        (1, 1, i16::MAX as i32 + 1, 0, "left above i16"),
        (1, 1, 0, i16::MIN as i32 - 1, "top below i16"),
        (1, 1, 0, i16::MAX as i32 + 1, "top above i16"),
    ];
    for (width, height, left, top, case) in invalid {
        assert!(
            packed(placement(width, height, left, top)).is_none(),
            "{case}"
        );
    }
}

/// Each non-drawing entry retires on its own last use rather than on
/// a shared tick. At frame 1024 with a 512-frame window, an entry
/// dies once `last_use + 512 + 1 <= 1024`, i.e. `last_use <= 511`.
#[test]
fn a_drained_ticket_retires_only_its_own_stale_empty() {
    let slots = vec![
        slot(None, 1),                          // 1 + 513 = 514 <= 1024 -> reclaimed
        slot(None, 511),                        // 511 + 513 = 1024 <= 1024 -> reclaimed
        slot(None, 512),                        // 512 + 513 = 1025 > 1024 -> re-filed
        slot(None, 1024),                       // freshly touched -> re-filed
        slot(Some(AllocId::deserialize(0)), 1), // allocated -> evict_one's job
    ];
    let mut cache = FxHashMap::default();
    for i in 0..slots.len() as u32 {
        cache.insert(key(i as u16 + 1), i);
    }
    let mut free = Vec::new();
    let refile = |cache: &mut FxHashMap<TestKey, u32>, free: &mut Vec<u32>, k| {
        retire_unallocated(cache, &slots, free, k, 1024)
    };

    assert_eq!(refile(&mut cache, &mut free, key(1)), None);
    assert_eq!(refile(&mut cache, &mut free, key(2)), None);
    assert_eq!(
        refile(&mut cache, &mut free, key(3)),
        Some(1025),
        "an entry still inside its window is re-filed for its own deadline",
    );
    assert_eq!(refile(&mut cache, &mut free, key(4)), Some(1537));
    assert_eq!(
        refile(&mut cache, &mut free, key(5)),
        None,
        "an allocated entry is not this wheel's business",
    );

    assert!(
        !cache.contains_key(&key(1)),
        "stale empty must be reclaimed"
    );
    assert!(!cache.contains_key(&key(2)), "boundary empty too");
    assert!(cache.contains_key(&key(3)), "one frame short of the window");
    assert!(cache.contains_key(&key(4)), "fresh empty survives");
    assert!(cache.contains_key(&key(5)), "allocated entry is untouched");
    // Reclaimed slab slots are handed back for reuse, in ticket order.
    assert_eq!(free, vec![0, 1]);

    // A second ticket for an already-reclaimed key is a no-op, not a
    // double free — that is what makes an early or duplicate fire safe.
    assert_eq!(refile(&mut cache, &mut free, key(1)), None);
    assert_eq!(free, vec![0, 1]);
}

/// Growth stops at the byte budget, not at whatever the adapter
/// happens to allow — the whole point of the ceiling.
///
/// The exactness matters: 16 MiB is `2^24` and both pixel sizes are
/// powers of two, so the ceiling lands on a power-of-two side and
/// the doubling sequence reaches it precisely rather than stopping
/// one short or clamping to an odd size.
#[test]
fn growth_stops_at_the_byte_budget_not_the_device_limit() {
    for device_max in [8192, 16384, 32768] {
        assert_eq!(
            growth_ceiling(device_max, ContentType::Mask),
            4096,
            "16 MiB of 1-byte pixels is 4096², whatever device_max={device_max} allows",
        );
        assert_eq!(
            growth_ceiling(device_max, ContentType::Color),
            2048,
            "16 MiB of 4-byte pixels is 2048², device_max={device_max}",
        );
    }
    // Both ceilings are exactly the budget, so neither wastes half a
    // doubling nor overshoots it.
    for (content, side) in [(ContentType::Mask, 4096u64), (ContentType::Color, 2048)] {
        let bytes = side * side * u64::from(content.bytes_per_pixel());
        assert_eq!(bytes, MAX_ATLAS_BYTE_BUDGET, "{content:?}");
    }
    // A device meaner than the budget still binds.
    assert_eq!(growth_ceiling(1024, ContentType::Mask), 1024);
    assert_eq!(growth_ceiling(512, ContentType::Color), 512);
}

/// The clock skips exactly three things — wrong content, no
/// rectangle to reclaim, and drawn on the current frame — and takes
/// the first survivor in hand order rather than the globally oldest.
///
/// The second half is the property the whole change turns on: the
/// hand *persists*. A second eviction resumes past the first victim
/// instead of restarting, which is what makes a run of evictions
/// cost one rotation between them all rather than a full slab walk
/// each. Slot 5 is deliberately older than slot 1, so an exact-LRU
/// picker would answer 5 first and this must not.
#[test]
fn the_clock_resumes_where_it_stopped_and_skips_ineligible_slots() {
    let slots = vec![
        slot(Some(AllocId::deserialize(0)), 8),
        slot(Some(AllocId::deserialize(1)), 2),
        slot(None, 1), // never drew — nothing to deallocate
        AtlasSlot {
            content: ContentType::Color,
            ..slot(Some(AllocId::deserialize(3)), 0)
        },
        slot(Some(AllocId::deserialize(4)), 10), // touched this frame
        slot(Some(AllocId::deserialize(5)), 1),  // the true LRU
    ];

    // From rest, the first eligible mask slot is 0 — not 5, which is
    // older. One step examined, and the hand parks past it.
    let first = clock_victim(&slots, 0, ContentType::Mask, 10);
    assert_eq!(
        first,
        ClockSweep {
            victim: Some(0),
            hand: 1,
            examined: 1,
        },
    );
    // Resuming from there takes slot 1 — again one step, because the
    // hand did not restart.
    let second = clock_victim(&slots, first.hand, ContentType::Mask, 10);
    assert_eq!(
        second,
        ClockSweep {
            victim: Some(1),
            hand: 2,
            examined: 1,
        },
    );
    // Now it must walk over the unallocated slot 2, the colour slot
    // 3, and the current-frame slot 4 to reach 5.
    let third = clock_victim(&slots, second.hand, ContentType::Mask, 10);
    assert_eq!(
        third,
        ClockSweep {
            victim: Some(5),
            hand: 0,
            examined: 4,
        },
    );
    // The colour side sees only its own slot, wherever the hand is.
    assert_eq!(
        clock_victim(&slots, 5, ContentType::Color, 10).victim,
        Some(3),
    );
    // Nothing eligible: one full rotation, no victim, and the hand
    // is left where it started so the next call is not skewed.
    // Frame 1, not 2 — slot 5's `last_use` of 1 still qualifies at
    // frame 2, and the oldest mask slot has to be *at* the frame for
    // the side to be genuinely dry.
    let dry = clock_victim(&slots, 2, ContentType::Mask, 1);
    assert_eq!(
        dry,
        ClockSweep {
            victim: None,
            hand: 2,
            examined: slots.len() as u32,
        },
    );
    // An empty slab is not a rotation over nothing.
    assert_eq!(
        clock_victim(&[], 7, ContentType::Mask, 10),
        ClockSweep {
            victim: None,
            hand: 0,
            examined: 0,
        },
    );
}
