//! The raster atlas's wire metadata, expiry of empty entries, and growth under
//! the byte budget.

use super::*;
use etagere::AllocId;
use glam::{I16Vec2, IVec2, UVec2};

/// The atlas is generic over its key, so its tests use the cheapest one that
/// satisfies the bounds; nothing depends on what a key means.
type TestKey = u16;

fn key(id: u16) -> TestKey {
    id
}

#[test]
fn packed_metadata_checks_every_wire_boundary() {
    let packed = |width, height, left, top| {
        PackedMetadata::new(UVec2::new(width, height), IVec2::new(left, top))
    };
    assert_eq!(
        packed(0, 0, 0, 0).unwrap(),
        PackedMetadata {
            size: U16Vec2::new(0, 0),
            bearing: I16Vec2::new(0, 0),
        }
    );
    assert_eq!(
        packed(
            u32::from(u16::MAX),
            u32::from(u16::MAX),
            i32::from(i16::MIN),
            i32::from(i16::MAX),
        )
        .unwrap(),
        PackedMetadata {
            size: U16Vec2::new(u16::MAX, u16::MAX),
            bearing: I16Vec2::new(i16::MIN, i16::MAX),
        }
    );
    assert_eq!(
        packed(1, 1, i32::from(i16::MAX), i32::from(i16::MIN)).unwrap(),
        PackedMetadata {
            size: U16Vec2::new(1, 1),
            bearing: I16Vec2::new(i16::MAX, i16::MIN),
        }
    );

    let invalid = [
        (u32::from(u16::MAX) + 1, 1, 0, 0, "width above u16"),
        (1, u32::from(u16::MAX) + 1, 0, 0, "height above u16"),
        (1, 1, i32::from(i16::MIN) - 1, 0, "left below i16"),
        (1, 1, i32::from(i16::MAX) + 1, 0, "left above i16"),
        (1, 1, 0, i32::from(i16::MIN) - 1, "top below i16"),
        (1, 1, 0, i32::from(i16::MAX) + 1, "top above i16"),
    ];
    for (width, height, left, top, case) in invalid {
        assert!(packed(width, height, left, top).is_none(), "{case}");
    }
}

/// Each non-drawing entry retires on its own last use, not a shared tick. At
/// frame 1024 with a 120-frame window an entry dies once
/// `last_use + 120 + 1 <= 1024`, i.e. `last_use <= 903`.
#[test]
fn a_drained_ticket_retires_only_its_own_stale_empty() {
    let mut slots = vec![
        AtlasSlot::for_test(None, 1),    // 1 + 121 = 122 <= 1024 -> reclaimed
        AtlasSlot::for_test(None, 903),  // 903 + 121 = 1024 <= 1024 -> reclaimed
        AtlasSlot::for_test(None, 904),  // 904 + 121 = 1025 > 1024 -> re-filed
        AtlasSlot::for_test(None, 1024), // freshly touched -> re-filed
        AtlasSlot::for_test(Some(AllocId::deserialize(0)), 1), // allocated -> evict_one's job
    ];
    let mut cache = FxHashMap::default();
    for i in 0..slots.len() as u32 {
        cache.insert(key(i as u16 + 1), i);
    }
    let mut free = FreeSlots::default();
    // No side: every entry here is non-drawing, so `FreeSlots::release` never needs a packer.
    let refile = |cache: &mut FxHashMap<TestKey, u32>,
                  slots: &mut [AtlasSlot],
                  free: &mut FreeSlots,
                  k| { retire_unallocated(cache, slots, &mut [], free, k, 1024) };

    assert_eq!(refile(&mut cache, &mut slots, &mut free, key(1)), None);
    assert_eq!(refile(&mut cache, &mut slots, &mut free, key(2)), None);
    assert_eq!(
        refile(&mut cache, &mut slots, &mut free, key(3)),
        Some(1025),
        "an entry still inside its window is re-filed for its own deadline",
    );
    assert_eq!(
        refile(&mut cache, &mut slots, &mut free, key(4)),
        Some(1145)
    );
    assert_eq!(
        refile(&mut cache, &mut slots, &mut free, key(5)),
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
    assert_eq!(free.as_slice(), [0, 1]);

    // A second ticket for a reclaimed key is a no-op, making an early or
    // duplicate fire safe.
    assert_eq!(refile(&mut cache, &mut slots, &mut free, key(1)), None);
    assert_eq!(free.as_slice(), [0, 1]);
}

/// Growth stops at the byte budget, not the adapter's limit. 16 MiB is `2^24`
/// and both pixel sizes are powers of two, so the ceiling is a power-of-two
/// side and the doubling sequence reaches it exactly.
#[test]
fn growth_stops_at_the_byte_budget_not_the_device_limit() {
    // What both tenants configure today; the budget is per instance.
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
    // Both ceilings are exactly the budget.
    for (content, side) in [(ContentType::Mask, 4096u64), (ContentType::Color, 2048)] {
        let bytes = side * side * u64::from(content.bytes_per_pixel());
        assert_eq!(bytes, BUDGET, "{content:?}");
    }
    // A device meaner than the budget still binds.
    assert_eq!(Side::growth_ceiling(1024, ContentType::Mask, BUDGET), 1024);
    assert_eq!(Side::growth_ceiling(512, ContentType::Color, BUDGET), 512);

    // The budget is per instance, so a tenant can buy more room without moving
    // the other's ceiling. Quadrupling the bytes doubles the side.
    assert_eq!(
        Side::growth_ceiling(16384, ContentType::Color, BUDGET * 4),
        4096
    );
    assert_eq!(
        Side::growth_ceiling(16384, ContentType::Mask, BUDGET / 4),
        2048
    );
}

/// The clock skips exactly three things (wrong content, no rectangle to
/// reclaim, drawn on the current frame) and takes the first survivor in hand
/// order, not the globally oldest.
///
/// The hand persists: a second eviction resumes past the first victim, so a
/// run of evictions costs one rotation between them, not a slab walk each.
/// Slot 5 is deliberately older than slot 1; an exact-LRU picker would answer
/// 5 first.
#[test]
fn the_clock_resumes_where_it_stopped_and_skips_ineligible_slots() {
    let slots = vec![
        AtlasSlot::for_test(Some(AllocId::deserialize(0)), 8),
        AtlasSlot::for_test(Some(AllocId::deserialize(1)), 2),
        AtlasSlot::for_test(None, 1), // never drew — nothing to deallocate
        AtlasSlot {
            placement: Some(SlotPlacement {
                content: ContentType::Color,
                ..SlotPlacement::for_test(AllocId::deserialize(3))
            }),
            ..AtlasSlot::for_test(None, 0)
        },
        AtlasSlot::for_test(Some(AllocId::deserialize(4)), 10), // touched this frame
        AtlasSlot::for_test(Some(AllocId::deserialize(5)), 1),  // the true LRU
    ];

    // From rest, the first eligible mask slot is 0, not the older 5. One step
    // examined; the hand parks past it.
    let first = ClockSweep::over(&slots, 0, ContentType::Mask, 10);
    assert_eq!(
        first,
        ClockSweep {
            victim: Some(0),
            hand: 1,
            examined: 1,
        },
    );
    // Resuming takes slot 1 in one step: the hand did not restart.
    let second = ClockSweep::over(&slots, first.hand, ContentType::Mask, 10);
    assert_eq!(
        second,
        ClockSweep {
            victim: Some(1),
            hand: 2,
            examined: 1,
        },
    );
    // Now it walks over unallocated slot 2, colour slot 3 and current-frame
    // slot 4 to reach 5.
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
    // Nothing eligible: one full rotation, no victim, hand left where it
    // started. Frame 1, not 2: slot 5's `last_use` of 1 still qualifies at
    // frame 2, and the oldest slot must be at the frame for the side to be dry.
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

/// The escalation ladder in [`RasterAtlas::allocate`], driven against a real
/// device because growing a side allocates a texture.
mod gpu {
    use super::*;
    use crate::gpu::test_gpu::headless_test_gpu;

    /// A mask side that starts at 128² and tops out at 256², so one insert can
    /// walk the whole ladder cheaply. `eager_growth_bytes` is zero because the
    /// eager arm belongs to
    /// [`growth_stops_at_the_byte_budget_not_the_device_limit`].
    fn small_atlas(device: &wgpu::Device) -> RasterAtlas<TestKey> {
        RasterAtlas::new(
            device,
            &RasterProgram::new(device),
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

    /// Insert `count` 16² mask entries, all stamped with the current frame.
    fn fill(atlas: &mut RasterAtlas<TestKey>, device: &wgpu::Device, count: u16) {
        let pixels = [0u8; 16 * 16];
        let metadata = PackedMetadata::new(UVec2::new(16, 16), IVec2::new(0, 0)).unwrap();
        for i in 0..count {
            assert!(
                atlas
                    .insert(device, key(i), ContentType::Mask, metadata, &pixels)
                    .is_some(),
                "16² entry {i} must fit a 128² side",
            );
        }
    }

    /// An entry larger than the side will ever be cannot fit by freeing
    /// rectangles, so the eviction loop must not run for it; otherwise one
    /// oversized glyph empties the whole side every frame it is asked for.
    #[test]
    fn an_entry_past_the_ceiling_is_refused_without_evicting_anything() {
        let gpu = headless_test_gpu();
        let mut atlas = small_atlas(&gpu.device);
        fill(&mut atlas, &gpu.device, 16);
        // Age every entry out of the current frame so all 16 are eligible
        // victims; otherwise the clock protects them and the test passes
        // for the wrong reason.
        atlas.advance_to(1);

        let metadata = PackedMetadata::new(UVec2::new(300, 300), IVec2::new(0, 0)).unwrap();
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

    /// A frame asking for more than its atlas holds must not pay a clock
    /// rotation per starving entry. Once every slot carries the current
    /// frame's stamp nothing is evictable until the clock advances, so the
    /// first empty rotation is the last worth walking; unmemoized it is
    /// quadratic in the slab.
    #[test]
    fn a_side_walked_dry_is_not_walked_again_until_the_clock_moves() {
        let gpu = headless_test_gpu();
        let mut atlas = small_atlas(&gpu.device);
        let pixels = [0u8; 16 * 16];
        let metadata = PackedMetadata::new(UVec2::new(16, 16), IVec2::new(0, 0)).unwrap();

        // Saturate the side: uniform 16² tiles shelf-pack a 256² side with no
        // waste (16 shelves of 16), so capacity is exact.
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

        // Redraw the whole working set next frame, as a real frame does before
        // starving: every slot is stamped current, none a victim.
        atlas.advance_to(1);
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
            u64::from(placed),
            "one rotation over the slab for the first refusal and none \
             for the seven after it — not eight rotations",
        );

        // The clock advancing makes the side worth walking again; now every slot is a victim.
        atlas.advance_to(2);
        let evicted_before = atlas.counters.evictions.count();
        assert!(
            atlas
                .insert(&gpu.device, key(2000), ContentType::Mask, metadata, &pixels)
                .is_some(),
            "an aged-out tile is evictable again",
        );
        // How many victims one tile costs is etagere's bucket granularity, so
        // the count is read, not pinned. The atlas owes the conservation law:
        // every entry that left did so through `evict_one`, so the resident set
        // shrank by exactly the evictions.
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

    /// [`RasterAtlas::forget`] retires a whole family of keys at once, which
    /// the clock cannot (a dead entry looks cold). Everything the predicate
    /// keeps comes through untouched, rectangles included.
    #[test]
    fn forget_retires_the_keys_it_rejects_and_nothing_else() {
        let gpu = headless_test_gpu();
        let mut atlas = small_atlas(&gpu.device);
        let pixels = [0u8; 16 * 16];
        let metadata = PackedMetadata::new(UVec2::new(16, 16), IVec2::new(0, 0)).unwrap();
        for i in 0..8 {
            atlas
                .insert(&gpu.device, key(i), ContentType::Mask, metadata, &pixels)
                .expect("eight 16² tiles fit a 128² side");
        }
        // A non-drawing entry too: only its expiry ticket would retire it.
        atlas.insert_unallocated(key(100));
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

        // A freed slab index still holds its old key in `slot_keys`; a walk
        // deciding liveness from that column would reclaim it twice and hand
        // one index to two inserts. This pass must find nothing: the map is
        // the only authority on which indices live.
        let free_before = atlas.free.as_slice().len();
        atlas.forget(|k| k % 2 == 0 && *k != 100);
        assert_eq!(
            atlas.free.as_slice().len(),
            free_before,
            "keys already retired must not be freed twice",
        );
        assert_eq!(atlas.cache.len(), 4, "and the live entries are untouched");

        // The reclaimed rectangles are back: the side had room for eight and
        // holds four, so four more land without a grow or eviction.
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

    /// An entry that fits the ceiling but not the current side must grow
    /// whatever the byte budget says: eviction frees rectangles but never
    /// widens the texture. The side extents bracket the insert, pinning that
    /// only the side that grew moves.
    #[test]
    fn an_entry_wider_than_the_side_grows_rather_than_evicting() {
        let gpu = headless_test_gpu();
        let mut atlas = small_atlas(&gpu.device);
        fill(&mut atlas, &gpu.device, 16);
        atlas.advance_to(1);

        assert_eq!(
            [
                atlas.side_px(ContentType::Mask),
                atlas.side_px(ContentType::Color)
            ],
            [128, 128],
            "both sides start at their configured 128²",
        );

        let pixels = vec![0u8; 200 * 200];
        let metadata = PackedMetadata::new(UVec2::new(200, 200), IVec2::new(0, 0)).unwrap();
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
        assert_eq!(
            [
                atlas.side_px(ContentType::Mask),
                atlas.side_px(ContentType::Color)
            ],
            [256, 128],
            "only the side that had to grow grew",
        );
    }
}
