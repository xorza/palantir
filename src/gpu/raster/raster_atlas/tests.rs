//! The tenant's side of the pool: its wire metadata, its rasters found where the pool holds them,
//! an eviction reaching it, its keys forgotten, and the expiry of its empty entries.

use super::*;
use crate::gpu::raster::raster_atlas::packed_metadata::PackedMetadata;
use glam::{IVec2, U16Vec2, UVec2};

/// The atlas is generic over its key, so its tests use the cheapest one that
/// satisfies the bounds; nothing depends on what a key means.
type TestKey = u16;

fn key(id: u16) -> TestKey {
    id
}

/// A pool whose lists hold four 32² pages each, opened at 32.
fn pool() -> AtlasPool {
    AtlasPool::uniform(32, 32, 0)
}

/// File a raster of `size` in `format`'s list under `key`, its bearing `(1, 2)`, as a pass files
/// one, and say where it draws.
fn insert(
    atlas: &mut RasterAtlas<TestKey>,
    pool: &mut AtlasPool,
    key: TestKey,
    format: AtlasFormat,
    size: UVec2,
) -> SlotPlacement {
    let allocation: Allocation = pool.allocate(format, atlas.tenant(), size).unwrap();
    atlas.insert_placed(key, allocation, I16Vec2::new(1, 2))
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

/// A raster is found where the pool holds it, with its bearing, its content its list's: a mask at
/// the origin of the mask list's first page, a color one at the color list's. A key never filed
/// misses, and one filed as drawing nothing is found as nothing.
#[test]
fn a_raster_is_found_where_the_pool_holds_it() {
    let mut pool = pool();
    let mut atlas = RasterAtlas::<TestKey>::new(Tenant::Glyphs);
    let mask = insert(
        &mut atlas,
        &mut pool,
        key(1),
        AtlasFormat::R8,
        UVec2::new(10, 12),
    );
    let color = insert(
        &mut atlas,
        &mut pool,
        key(2),
        AtlasFormat::Rgba8,
        UVec2::splat(4),
    );
    assert_eq!(
        mask,
        SlotPlacement {
            handle: mask.handle,
            page: 0,
            origin: U16Vec2::ZERO,
            size: U16Vec2::new(10, 12),
            bearing: I16Vec2::new(1, 2),
        }
    );
    assert_eq!(mask.content(), ContentType::Mask);
    assert_eq!(
        (color.page, color.origin, color.content()),
        (0, U16Vec2::ZERO, ContentType::Color)
    );
    assert_eq!(atlas.find(&mut pool, &key(1)), Some(Found::Raster(mask)));
    assert_eq!(atlas.find(&mut pool, &key(3)), None);
    atlas.insert_unplaced(key(4), 0);
    assert_eq!(atlas.find(&mut pool, &key(4)), Some(Found::Nothing));
    assert_eq!(atlas.cached_len(), 3);
}

/// With every mask page full of glyphs no frame drew this frame or the last, two icons evict two
/// of them. The glyph tenant learns of one at its lookup, which misses and drops the key, and of
/// the other at its drain; both count, both handles read as stale in the pool, so a run that
/// named either replays nothing, and the other two glyphs stay.
#[test]
fn an_eviction_reaches_its_tenant() {
    let mut pool = pool();
    let mut glyphs = RasterAtlas::<TestKey>::new(Tenant::Glyphs);
    let mut icons = RasterAtlas::<TestKey>::new(Tenant::Icons);
    let full = UVec2::splat(32);
    let held: Vec<SlotPlacement> = (0..4)
        .map(|id| insert(&mut glyphs, &mut pool, key(id), AtlasFormat::R8, full))
        .collect();
    pool.advance_to(2);
    for id in 0..2 {
        insert(&mut icons, &mut pool, key(id), AtlasFormat::R8, full);
    }
    assert_eq!(
        glyphs.find(&mut pool, &key(0)),
        None,
        "evicted, found at the lookup"
    );
    glyphs.drop_evicted(&mut pool);
    assert_eq!(glyphs.cached_len(), 2);
    assert_eq!(glyphs.counters.evictions.count(), 2);
    for placement in &held[..2] {
        assert_eq!(pool.stamp(placement.handle), None);
    }
    for (id, placement) in (2..4).zip(&held[2..]) {
        assert_eq!(
            glyphs.find(&mut pool, &key(id)),
            Some(Found::Raster(*placement))
        );
    }
}

/// Forgetting a family of keys gives their rasters back to the pool: the page's live texels drop
/// by exactly the forgotten raster's, `8 · 8 = 64`, and its handle reads as stale.
#[test]
fn forget_gives_its_rasters_back_to_the_pool() {
    let mut pool = pool();
    let mut icons = RasterAtlas::<TestKey>::new(Tenant::Icons);
    let kept = insert(
        &mut icons,
        &mut pool,
        key(1),
        AtlasFormat::R8,
        UVec2::splat(4),
    );
    let gone = insert(
        &mut icons,
        &mut pool,
        key(2),
        AtlasFormat::R8,
        UVec2::splat(8),
    );
    let live = |pool: &AtlasPool| pool.list(AtlasFormat::R8).page(0).unwrap().live_texels();
    assert_eq!(live(&pool), 16 + 64);
    icons.forget(&mut pool, |&id| id == key(1));
    assert_eq!(live(&pool), 16);
    assert_eq!(pool.placement(gone.handle), None);
    assert_eq!(icons.cached_len(), 1);
    assert_eq!(icons.find(&mut pool, &key(1)), Some(Found::Raster(kept)));
}

/// Each non-drawing entry retires on its own last use, not a shared tick. At display frame 1024
/// with a 120-frame window an entry dies once `last_use + 120 + 1 <= 1024`, i.e.
/// `last_use <= 903`.
#[test]
fn a_drained_ticket_retires_only_its_own_stale_empty() {
    let placed = AtlasSlot::Placed(PlacedRaster {
        handle: AtlasHandle {
            format: AtlasFormat::R8,
            idx: 0,
            generation: 0,
        },
        bearing: I16Vec2::ZERO,
    });
    let unplaced = |last_use| AtlasSlot::Unplaced { last_use };
    let mut slots = vec![
        unplaced(1),    // 1 + 121 = 122 <= 1024 -> reclaimed
        unplaced(903),  // 903 + 121 = 1024 <= 1024 -> reclaimed
        unplaced(904),  // 904 + 121 = 1025 > 1024 -> re-filed
        unplaced(1024), // freshly found -> re-filed
        placed,         // placed -> the pool's to reclaim
    ];
    let mut cache = FxHashMap::default();
    for i in 0..slots.len() as u32 {
        cache.insert(key(i as u16 + 1), i);
    }
    let mut free = Vec::new();
    let mut slab = Slab {
        cache: &mut cache,
        slots: &mut slots,
        free: &mut free,
    };

    assert_eq!(slab.retire_unallocated(key(1), 1024), None);
    assert_eq!(slab.retire_unallocated(key(2), 1024), None);
    assert_eq!(
        slab.retire_unallocated(key(3), 1024),
        Some(1025),
        "an entry still inside its window is re-filed for its own deadline",
    );
    assert_eq!(slab.retire_unallocated(key(4), 1024), Some(1145));
    assert_eq!(
        slab.retire_unallocated(key(5), 1024),
        None,
        "a placed entry is not this wheel's business",
    );

    assert!(
        !cache.contains_key(&key(1)),
        "stale empty must be reclaimed"
    );
    assert!(!cache.contains_key(&key(2)), "boundary empty too");
    assert!(cache.contains_key(&key(3)), "one frame short of the window");
    assert!(cache.contains_key(&key(4)), "fresh empty survives");
    assert!(cache.contains_key(&key(5)), "placed entry is untouched");
    // Reclaimed slab slots are handed back for reuse, in ticket order.
    assert_eq!(free, [0, 1]);

    // A second ticket for a reclaimed key is a no-op, making an early or
    // duplicate fire safe.
    let mut slab = Slab {
        cache: &mut cache,
        slots: &mut slots,
        free: &mut free,
    };
    assert_eq!(slab.retire_unallocated(key(1), 1024), None);
    assert_eq!(free, [0, 1]);
}
