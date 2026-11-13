//! The tenant's side of the pool: its wire metadata, its rasters filed and found, an eviction
//! reaching it, its keys forgotten, and the expiry of its empty entries.

use super::*;
use crate::gpu::atlas::page_list::PageListConfig;
use glam::{I16Vec2, IVec2};

/// The atlas is generic over its key, so its tests use the cheapest one that
/// satisfies the bounds; nothing depends on what a key means.
type TestKey = u16;

fn key(id: u16) -> TestKey {
    id
}

/// A pool whose lists hold four 32² pages each, opened at 32.
fn pool() -> AtlasPool {
    let config = PageListConfig {
        first_edge: 32,
        page_edge: 32,
        soft_bytes: 0,
    };
    AtlasPool::with_configs([config; 3], 16384)
}

/// A raster of `content` and `size`, its bearing `(1, 2)`, its bytes zero.
fn raster(content: ContentType, size: U16Vec2, pixels: &mut Vec<u8>) -> PackedRaster<'_> {
    pixels.clear();
    pixels.resize(
        usize::from(size.x) * usize::from(size.y) * content.bytes_per_pixel() as usize,
        0,
    );
    PackedRaster {
        metadata: PackedMetadata::new(size.as_uvec2(), IVec2::new(1, 2)).unwrap(),
        content,
        pixels,
    }
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

/// A mask raster files in the pool's mask list and a color one in its color list, each at the
/// origin of its first page, with its bearing and content; a touch finds each, and each queues its
/// upload for its own list. A raster past a page is counted and takes a slot that draws nothing,
/// which a touch hits, so its rasterizer runs once.
#[test]
fn a_raster_files_in_its_list_and_a_touch_finds_it() {
    let mut pool = pool();
    let mut atlas = RasterAtlas::<TestKey>::new(Tenant::Glyphs, "test");
    let mut pixels = Vec::new();
    let mask = atlas
        .insert(
            &mut pool,
            key(1),
            raster(ContentType::Mask, U16Vec2::new(10, 12), &mut pixels),
        )
        .unwrap();
    let color = atlas
        .insert(
            &mut pool,
            key(2),
            raster(ContentType::Color, U16Vec2::new(4, 4), &mut pixels),
        )
        .unwrap();
    let mask_handle = atlas.placement(&pool, mask).unwrap().handle;
    assert_eq!(mask_handle.format, AtlasFormat::R8);
    assert_eq!(
        atlas.placement(&pool, mask),
        Some(SlotPlacement {
            handle: mask_handle,
            page: 0,
            origin: U16Vec2::ZERO,
            size: U16Vec2::new(10, 12),
            bearing: I16Vec2::new(1, 2),
            content: ContentType::Mask,
        })
    );
    assert_eq!(
        atlas
            .placement(&pool, color)
            .map(|p| (p.page, p.origin, p.content)),
        Some((0, U16Vec2::ZERO, ContentType::Color))
    );
    assert_eq!(atlas.touch(&mut pool, &key(1)), Some(mask));
    assert_eq!(atlas.touch(&mut pool, &key(3)), None);
    assert_eq!(
        atlas
            .pending_copies
            .iter()
            .map(|copy| (copy.format, copy.size))
            .collect::<Vec<_>>(),
        [
            (AtlasFormat::R8, UVec2::new(10, 12)),
            (AtlasFormat::Rgba8, UVec2::new(4, 4))
        ]
    );
    let wide = atlas
        .insert(
            &mut pool,
            key(4),
            raster(ContentType::Mask, U16Vec2::new(33, 1), &mut pixels),
        )
        .expect("a raster past the page takes a slot");
    assert_eq!(atlas.counters.oversized.count(), 1);
    assert_eq!(atlas.placement(&pool, wide), None, "which draws nothing");
    assert_eq!(atlas.touch(&mut pool, &key(4)), Some(wide), "and hits");
    assert_eq!(atlas.pending_copies.len(), 2, "with no upload");
}

/// With every mask page full of glyphs no frame drew this frame or the last, two icons evict two
/// of them. The glyph tenant learns of one at its lookup, which misses and drops the key, and of
/// the other at its drain; both count, both handles read as stale in the pool, so a run that
/// named either replays nothing, and the other two glyphs stay.
#[test]
fn an_eviction_reaches_its_tenant() {
    let mut pool = pool();
    let mut glyphs = RasterAtlas::<TestKey>::new(Tenant::Glyphs, "glyphs");
    let mut icons = RasterAtlas::<TestKey>::new(Tenant::Icons, "icons");
    let mut pixels = Vec::new();
    let full = U16Vec2::splat(32);
    let held: Vec<(u32, AtlasHandle)> = (0..4)
        .map(|id| {
            let idx = glyphs
                .insert(
                    &mut pool,
                    key(id),
                    raster(ContentType::Mask, full, &mut pixels),
                )
                .unwrap();
            (idx, glyphs.placement(&pool, idx).unwrap().handle)
        })
        .collect();
    pool.advance_to(2);
    for id in 0..2 {
        assert!(
            icons
                .insert(
                    &mut pool,
                    key(id),
                    raster(ContentType::Mask, full, &mut pixels)
                )
                .is_some()
        );
    }
    assert_eq!(
        glyphs.touch(&mut pool, &key(0)),
        None,
        "evicted, found at the lookup"
    );
    glyphs.end_frame(&mut pool, 1);
    assert_eq!(glyphs.cached_len(), 2);
    assert_eq!(glyphs.counters.evictions.count(), 2);
    for &(_, handle) in &held[..2] {
        assert_eq!(pool.stamp(handle), None);
    }
    for (id, &(idx, handle)) in (2..4).zip(&held[2..]) {
        assert_eq!(glyphs.touch(&mut pool, &key(id)), Some(idx));
        assert!(pool.stamp(handle).is_some());
    }
}

/// Forgetting a family of keys gives their rasters back to the pool: the page's live texels drop
/// by exactly the forgotten raster's, `8 · 8 = 64`, and its handle reads as stale.
#[test]
fn forget_gives_its_rasters_back_to_the_pool() {
    let mut pool = pool();
    let mut icons = RasterAtlas::<TestKey>::new(Tenant::Icons, "icons");
    let mut pixels = Vec::new();
    let kept = icons
        .insert(
            &mut pool,
            key(1),
            raster(ContentType::Mask, U16Vec2::splat(4), &mut pixels),
        )
        .unwrap();
    let gone = icons
        .insert(
            &mut pool,
            key(2),
            raster(ContentType::Mask, U16Vec2::splat(8), &mut pixels),
        )
        .unwrap();
    let handle = icons.placement(&pool, gone).unwrap().handle;
    let live = || pool.list(AtlasFormat::R8).page(0).unwrap().live_texels();
    assert_eq!(live(), 16 + 64);
    icons.forget(&mut pool, |&id| id == key(1));
    assert_eq!(
        pool.list(AtlasFormat::R8).page(0).unwrap().live_texels(),
        16
    );
    assert_eq!(pool.placement(handle), None);
    assert_eq!(icons.cached_len(), 1);
    assert!(icons.placement(&pool, kept).is_some());
}

/// Each non-drawing entry retires on its own last use, not a shared tick. At
/// frame 1024 with a 120-frame window an entry dies once
/// `last_use + 120 + 1 <= 1024`, i.e. `last_use <= 903`.
#[test]
fn a_drained_ticket_retires_only_its_own_stale_empty() {
    let placed = AtlasSlot {
        state: SlotState::Placed(PlacedRaster {
            handle: AtlasHandle {
                format: AtlasFormat::R8,
                idx: 0,
                generation: 0,
            },
            bearing: I16Vec2::ZERO,
            content: ContentType::Mask,
        }),
        last_use: 1,
    };
    let mut slots = vec![
        AtlasSlot::unplaced(1),    // 1 + 121 = 122 <= 1024 -> reclaimed
        AtlasSlot::unplaced(903),  // 903 + 121 = 1024 <= 1024 -> reclaimed
        AtlasSlot::unplaced(904),  // 904 + 121 = 1025 > 1024 -> re-filed
        AtlasSlot::unplaced(1024), // freshly touched -> re-filed
        placed,                    // placed -> the pool's to reclaim
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
