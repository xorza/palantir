use super::*;
use etagere::AllocId;

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
/// a shared tick. At frame 1024 with a 120-frame window, an entry
/// dies once `last_use + 120 + 1 <= 1024`, i.e. `last_use <= 903`.
#[test]
fn a_drained_ticket_retires_only_its_own_stale_empty() {
    let slots = vec![
        slot(None, 1),                          // 1 + 121 = 122 <= 1024 -> reclaimed
        slot(None, 903),                        // 903 + 121 = 1024 <= 1024 -> reclaimed
        slot(None, 904),                        // 904 + 121 = 1025 > 1024 -> re-filed
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
    assert_eq!(refile(&mut cache, &mut free, key(4)), Some(1145));
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
    // What both tenants configure today. Per-instance now, so this pins the
    // arithmetic rather than a shared constant.
    const BUDGET: u64 = 16 << 20;
    for device_max in [8192, 16384, 32768] {
        assert_eq!(
            Side::growth_ceiling(device_max, ContentType::Mask, BUDGET),
            4096,
            "16 MiB of 1-byte pixels is 4096², whatever device_max={device_max} allows",
        );
        assert_eq!(
            Side::growth_ceiling(device_max, ContentType::Color, BUDGET),
            2048,
            "16 MiB of 4-byte pixels is 2048², device_max={device_max}",
        );
    }
    // Both ceilings are exactly the budget, so neither wastes half a
    // doubling nor overshoots it.
    for (content, side) in [(ContentType::Mask, 4096u64), (ContentType::Color, 2048)] {
        let bytes = side * side * u64::from(content.bytes_per_pixel());
        assert_eq!(bytes, BUDGET, "{content:?}");
    }
    // A device meaner than the budget still binds.
    assert_eq!(Side::growth_ceiling(1024, ContentType::Mask, BUDGET), 1024);
    assert_eq!(Side::growth_ceiling(512, ContentType::Color, BUDGET), 512);

    // The budget is per instance, so a tenant can buy itself more room
    // without moving the other's ceiling. Quadrupling the bytes doubles the
    // side, which is the relationship a caller has to reason about.
    assert_eq!(
        Side::growth_ceiling(16384, ContentType::Color, BUDGET * 4),
        4096
    );
    assert_eq!(
        Side::growth_ceiling(16384, ContentType::Mask, BUDGET / 4),
        2048
    );
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
    let first = ClockSweep::over(&slots, 0, ContentType::Mask, 10);
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
    let second = ClockSweep::over(&slots, first.hand, ContentType::Mask, 10);
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
    let third = ClockSweep::over(&slots, second.hand, ContentType::Mask, 10);
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
        ClockSweep::over(&slots, 5, ContentType::Color, 10).victim,
        Some(3),
    );
    // Nothing eligible: one full rotation, no victim, and the hand
    // is left where it started so the next call is not skewed.
    // Frame 1, not 2 — slot 5's `last_use` of 1 still qualifies at
    // frame 2, and the oldest mask slot has to be *at* the frame for
    // the side to be genuinely dry.
    let dry = ClockSweep::over(&slots, 2, ContentType::Mask, 1);
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
        ClockSweep::over(&[], 7, ContentType::Mask, 10),
        ClockSweep {
            victim: None,
            hand: 0,
            examined: 0,
        },
    );
}

/// The escalation ladder in [`RasterAtlas::allocate`], driven against a
/// real device because growing a side allocates a texture.
///
/// Gated on `internals` rather than bare `test` so a default headless
/// `cargo test` stays GPU-free, matching the text backend's own suite.
#[cfg(feature = "internals")]
mod gpu {
    use super::*;
    use crate::host::test_gpu::headless_test_gpu;
    use crate::renderer::backend::raster_atlas::RasterAtlasConfig;

    /// A mask side that starts at 128² and tops out at 256², so one
    /// insert can walk the whole ladder — fits, grows, or is refused —
    /// without allocating anything a test machine notices.
    ///
    /// `eager_growth_bytes` is zero on purpose: the eager arm is
    /// [`growth_stops_at_the_byte_budget_not_the_device_limit`]'s
    /// business, and leaving it on here would hide which arm answered.
    fn small_atlas(device: &wgpu::Device) -> RasterAtlas<TestKey> {
        RasterAtlas::new(
            device,
            RasterAtlasConfig {
                label: "palantir.test",
                initial_mask_px: 128,
                initial_color_px: 128,
                // 64 KiB is 256² of 1-byte mask and 128² of 4-byte colour.
                max_bytes: 256 * 256,
                eager_growth_bytes: 0,
            },
        )
    }

    /// Insert `count` 16² mask entries, all stamped with the current
    /// frame.
    fn fill(atlas: &mut RasterAtlas<TestKey>, device: &wgpu::Device, count: u16) {
        let pixels = [0u8; 16 * 16];
        let metadata = PackedMetadata::new(16, 16, 0, 0).unwrap();
        for i in 0..count {
            assert!(
                atlas
                    .insert(device, key(i), ContentType::Mask, metadata, &pixels)
                    .is_some(),
                "16² entry {i} must fit a 128² side",
            );
        }
    }

    /// An entry taller or wider than the side will *ever* be cannot be
    /// made to fit by freeing rectangles, so the eviction loop must not
    /// run for it. Left unguarded this is the worst thrash the atlas can
    /// reach: one oversized glyph — a canvas zoomed past the mask
    /// ceiling, an emoji past the colour one — empties the whole side
    /// every frame it is asked for, and the run it belongs to is refused
    /// as a template, so it *is* asked for again next frame.
    #[test]
    fn an_entry_past_the_ceiling_is_refused_without_evicting_anything() {
        let gpu = headless_test_gpu();
        let mut atlas = small_atlas(&gpu.device);
        fill(&mut atlas, &gpu.device, 16);
        // Age every entry out of the current frame so all 16 are
        // eligible victims — otherwise the clock would protect them and
        // the test would pass for the wrong reason.
        atlas.end_frame(1);

        let metadata = PackedMetadata::new(300, 300, 0, 0).unwrap();
        assert_eq!(
            atlas.insert(&gpu.device, key(999), ContentType::Mask, metadata, &[]),
            None,
            "300² cannot fit a side whose ceiling is 256²",
        );
        assert_eq!(
            atlas.cache.len(),
            16,
            "a refused entry must leave the resident set alone",
        );
    }

    /// A frame that asks for more than its atlas holds must not pay a
    /// clock rotation per starving entry.
    ///
    /// Once every slot of a side carries the current frame's stamp,
    /// nothing can become evictable until the clock advances — so the
    /// first rotation that comes up empty is the last one worth walking.
    /// Unmemoized this is O(slab) per starving entry, quadratic in the
    /// slab, and it lands on exactly the frame already too busy to draw
    /// what it was asked for.
    #[test]
    fn a_side_walked_dry_is_not_walked_again_until_the_clock_moves() {
        let gpu = headless_test_gpu();
        let mut atlas = small_atlas(&gpu.device);
        let pixels = [0u8; 16 * 16];
        let metadata = PackedMetadata::new(16, 16, 0, 0).unwrap();

        // Saturate the side. Uniform 16² tiles shelf-pack a 256² side
        // with no waste — 16 shelves of 16 — so the capacity is exact
        // rather than a property of etagere's packing heuristics.
        let mut placed = 0u16;
        while atlas
            .insert(
                &gpu.device,
                key(placed),
                ContentType::Mask,
                metadata,
                &pixels,
            )
            .is_some()
        {
            placed += 1;
        }
        assert_eq!(placed, 256);
        assert_eq!(atlas.slots.len(), placed as usize, "no evictions yet");

        // Redraw the whole working set on the next frame, which is what
        // a real frame does before it starts starving: every slot is now
        // stamped with the current frame and none of them is a victim.
        atlas.end_frame(1);
        for i in 0..placed {
            assert!(atlas.touch(&key(i)).is_some(), "tile {i} is resident");
        }

        let before = *atlas.counters.evict_scans.get();
        for extra in 0..8 {
            assert_eq!(
                atlas.insert(
                    &gpu.device,
                    key(1000 + extra),
                    ContentType::Mask,
                    metadata,
                    &pixels,
                ),
                None,
                "the side is full of entries drawn this frame",
            );
        }
        assert_eq!(
            *atlas.counters.evict_scans.get() - before,
            placed as u64,
            "one rotation over the slab for the first refusal and none \
             for the seven after it — not eight rotations",
        );

        // The clock advancing is what makes the side worth walking
        // again, and now every slot is a victim, so the entry lands.
        atlas.end_frame(2);
        let evicted_before = atlas.counters.evictions.count();
        assert!(
            atlas
                .insert(&gpu.device, key(2000), ContentType::Mask, metadata, &pixels)
                .is_some(),
            "an aged-out tile is evictable again",
        );
        // How *many* victims one tile costs is etagere's bucket
        // granularity — a bucket only returns its shelf space once every
        // item in it is gone — so the count is read rather than pinned.
        // What this atlas owes is the conservation law around it: every
        // entry that left, left through `evict_one`, so the resident set
        // shrank by exactly the evictions and not by a wipe.
        let evicted = atlas.counters.evictions.count() - evicted_before;
        assert!(
            evicted > 0,
            "the side was full — the tile had to displace something"
        );
        assert_eq!(
            atlas.cache.len(),
            placed as usize - evicted as usize + 1,
            "everything not evicted is still resident",
        );
    }

    /// [`RasterAtlas::forget`] retires a whole family of keys at once —
    /// what the clock cannot do, because a dead entry looks exactly like a
    /// cold one to it. Everything the predicate keeps must come through
    /// untouched, rectangles included.
    #[test]
    fn forget_retires_the_keys_it_rejects_and_nothing_else() {
        let gpu = headless_test_gpu();
        let mut atlas = small_atlas(&gpu.device);
        let pixels = [0u8; 16 * 16];
        let metadata = PackedMetadata::new(16, 16, 0, 0).unwrap();
        for i in 0..8 {
            atlas
                .insert(&gpu.device, key(i), ContentType::Mask, metadata, &pixels)
                .expect("eight 16² tiles fit a 128² side");
        }
        // A non-drawing entry too: it owns no rectangle, so only its
        // expiry ticket would ever have retired it.
        atlas.insert_unallocated(key(100), ContentType::Mask, PackedMetadata::EMPTY);
        assert_eq!(atlas.cache.len(), 9);

        // Keep the even keys and the empty; drop the odd ones.
        atlas.forget(|k| k % 2 == 0);
        assert_eq!(atlas.cache.len(), 5, "four odd keys retired");
        for i in 0..8u16 {
            assert_eq!(
                atlas.touch(&key(i)).is_some(),
                i % 2 == 0,
                "key {i} resident-ness",
            );
        }
        assert!(atlas.touch(&key(100)).is_some(), "the empty was kept");

        // A rejected empty goes too, and its slab index is reusable.
        atlas.forget(|k| *k != 100);
        assert!(atlas.touch(&key(100)).is_none());
        assert_eq!(atlas.cache.len(), 4);

        // A freed slab index still holds its old key in `slot_keys`, so
        // a walk that decided liveness from that column would reclaim it
        // a second time and hand one index to two future inserts. This
        // pass rejects every key already retired above and must find
        // nothing: the map is the only authority on which indices live.
        let free_before = atlas.free.len();
        atlas.forget(|k| k % 2 == 0 && *k != 100);
        assert_eq!(
            atlas.free.len(),
            free_before,
            "keys already retired must not be freed twice",
        );
        assert_eq!(atlas.cache.len(), 4, "and the live entries are untouched");

        // The reclaimed rectangles are genuinely back: the side had room
        // for eight and holds four, so four more must land without a grow
        // or an eviction.
        let before = atlas.counters.evictions.count();
        for i in 200..204u16 {
            assert!(
                atlas
                    .insert(&gpu.device, key(i), ContentType::Mask, metadata, &pixels)
                    .is_some(),
                "forget must have handed the rectangles back",
            );
        }
        assert_eq!(
            atlas.counters.evictions.count(),
            before,
            "the refills came out of reclaimed space, not out of victims",
        );
    }

    /// An entry that fits the ceiling but not the *current* side has to
    /// grow, whatever the byte budget says: eviction frees rectangles
    /// and never widens the texture, so every victim it takes is
    /// spent for nothing.
    ///
    /// Also the one place a grow's effect on the group-0 binding is
    /// observable: the params bracket the insert, so they pin both the
    /// lane order and that only the side that grew moves.
    #[test]
    fn an_entry_wider_than_the_side_grows_rather_than_evicting() {
        let gpu = headless_test_gpu();
        let mut atlas = small_atlas(&gpu.device);
        fill(&mut atlas, &gpu.device, 16);
        atlas.end_frame(1);

        assert_eq!(
            atlas.atlas_px(),
            [128, 128],
            "both sides start at their configured 128², reported `[color, mask]`",
        );

        let pixels = vec![0u8; 200 * 200];
        let metadata = PackedMetadata::new(200, 200, 0, 0).unwrap();
        assert!(
            atlas
                .insert(&gpu.device, key(999), ContentType::Mask, metadata, &pixels)
                .is_some(),
            "200² fits once the 128² side has grown to its 256² ceiling",
        );
        assert_eq!(
            atlas.cache.len(),
            17,
            "growing is what made room, so nothing should have been evicted",
        );
        // The invariant `BoundSides` exists for.
        assert_eq!(
            atlas.atlas_px(),
            [128, 256],
            "the grown mask side must be reflected in the params, the \
             untouched colour side must not",
        );
    }
}
