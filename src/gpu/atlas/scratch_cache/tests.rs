use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::atlas::Evictions;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::scratch_cache::{EvictedScratch, ScratchCache, approx_side};
use crate::gpu::atlas::tenant::{ItemOwner, Tenant};
use glam::UVec2;

/// A half-float cache on a device of 16384.
fn cache() -> ScratchCache {
    ScratchCache::new(AtlasFormat::Rgba16F, 16384)
}

/// The ladder: at least 16, powers of two through 1024, then the midpoint where it holds the
/// side: 1025 and 1536 take 1536, 1537 and 2048 take 2048, and 1936 × 1088, a full-window glass
/// with its apron, takes 2048 × 1536.
#[test]
fn a_side_rounds_up_the_ladder() {
    let rungs = [
        (1, 16),
        (16, 16),
        (17, 32),
        (1000, 1024),
        (1024, 1024),
        (1025, 1536),
        (1536, 1536),
        (1537, 2048),
        (2048, 2048),
        (3000, 3072),
        (3073, 4096),
    ];
    for (side, rung) in rungs {
        assert_eq!(approx_side(side), rung, "side {side}");
    }
    assert_eq!((approx_side(1936), approx_side(1088)), (2048, 1536));
}

/// A request rounds, then reuses an idle texture that covers it within twice the area, the
/// most recently idled first: 1300 × 900 rounds to 1536 × 1024 and takes an idle
/// 2048 × 1536, exactly twice its area, but not an idle 2048², past twice, which stays idle; a
/// held texture serves no one. A released id reads as stale.
#[test]
fn a_request_reuses_what_covers_it_within_twice_its_area() {
    let mut cache = cache();
    let square = cache
        .acquire(UVec2::splat(2048), Tenant::Backdrops.owner(), 0)
        .unwrap();
    let wide = cache
        .acquire(UVec2::new(1937, 1100), Tenant::Backdrops.owner(), 0)
        .unwrap();
    assert_eq!(cache.size(wide), Some(UVec2::new(2048, 1536)));
    let held = cache
        .acquire(UVec2::new(2048, 1536), Tenant::Backdrops.owner(), 0)
        .unwrap();
    cache.release(square, 1);
    cache.release(wide, 2);
    assert_eq!(cache.size(wide), None, "a released id is stale");
    let reused = cache
        .acquire(UVec2::new(1300, 900), Tenant::Backdrops.owner(), 0)
        .unwrap();
    assert_eq!(reused.idx, wide.idx, "the idle 2048 × 1536, twice the area");
    assert_eq!(cache.size(reused), Some(UVec2::new(2048, 1536)));
    let fresh = cache
        .acquire(UVec2::new(1300, 900), Tenant::Backdrops.owner(), 0)
        .unwrap();
    assert_ne!(
        fresh.idx, square.idx,
        "the idle 2048² passes twice the area"
    );
    assert_eq!(cache.size(fresh), Some(UVec2::new(1536, 1024)));
    assert_ne!(fresh.idx, held.idx);
    // 2048² + 2048 × 1536 twice + 1536 × 1024: the idle square counts until it expires.
    assert_eq!(cache.texels(), 2048 * 2048 + 2 * 2048 * 1536 + 1536 * 1024);
}

/// An idle texture goes an expiry window after it idled, `since + EXPIRY_FRAMES`, and not a
/// frame before; its slot serves the next new texture. A purge drops every idle texture at
/// once and keeps a held one its frame drew.
#[test]
fn an_idle_texture_expires_after_a_window() {
    let mut cache = cache();
    let a = cache
        .acquire(UVec2::splat(100), Tenant::Backdrops.owner(), 0)
        .unwrap();
    let b = cache
        .acquire(UVec2::splat(200), Tenant::Backdrops.owner(), 0)
        .unwrap();
    cache.release(a, 10);
    cache.expire(10 + EXPIRY_FRAMES - 1);
    assert_eq!(cache.texels(), 128 * 128 + 256 * 256, "a frame short");
    cache.expire(10 + EXPIRY_FRAMES);
    assert_eq!(cache.texels(), 256 * 256);
    let c = cache
        .acquire(UVec2::splat(30), Tenant::Backdrops.owner(), 0)
        .unwrap();
    assert_eq!(c.idx, a.idx, "the gone slot takes the new texture");
    cache.release(c, 20);
    assert_eq!(cache.stamp(b, 20), Some(UVec2::splat(256)));
    let mut evictions = Evictions::default();
    cache.purge(20, &mut evictions);
    assert_eq!(cache.texels(), 256 * 256, "the one drawn this frame stays");
    assert_eq!(cache.size(b), Some(UVec2::splat(256)));
    assert!(evictions.evicted_scratch(Tenant::Backdrops).is_empty());
}

/// A purge takes a held texture no item drew in its display frame, as it takes an item of a
/// page, and queues its id for its tenant with the tag it was acquired under; the id reads as
/// stale. Of two held at frame 3, the one drawn at 5 stays through a purge at 5, and the other,
/// tagged 7, goes.
#[test]
fn a_purge_takes_a_held_texture_its_frame_did_not_draw() {
    let mut cache = cache();
    let drawn = cache
        .acquire(UVec2::splat(100), Tenant::Backdrops.owner(), 3)
        .unwrap();
    let tagged = ItemOwner {
        tenant: Tenant::Backdrops,
        tag: 7,
    };
    let idle = cache.acquire(UVec2::splat(200), tagged, 3).unwrap();
    assert!(cache.stamp(drawn, 5).is_some());
    let mut evictions = Evictions::default();
    cache.purge(5, &mut evictions);
    assert_eq!(
        evictions.evicted_scratch(Tenant::Backdrops),
        [EvictedScratch { id: idle, tag: 7 }]
    );
    assert_eq!(cache.size(idle), None);
    assert_eq!(cache.stamp(idle, 5), None);
    assert_eq!(cache.texels(), 128 * 128);
    assert_eq!(cache.size(drawn), Some(UVec2::splat(128)));
}

#[test]
#[should_panic(expected = "a scratch id released twice")]
fn a_scratch_id_is_released_once() {
    let mut cache = cache();
    let id = cache
        .acquire(UVec2::splat(100), Tenant::Backdrops.owner(), 0)
        .unwrap();
    cache.release(id, 1);
    cache.release(id, 2);
}

#[test]
#[should_panic(expected = "a scratch id of another list")]
fn a_scratch_id_goes_back_to_its_own_list() {
    let id = cache()
        .acquire(UVec2::splat(100), Tenant::Backdrops.owner(), 0)
        .unwrap();
    ScratchCache::new(AtlasFormat::R8, 16384).release(id, 1);
}

/// No side passes the device's edge: a request wider than it is refused, and one within it
/// whose rung passes it takes the edge itself.
#[test]
fn no_side_passes_the_device_edge() {
    let mut cache = ScratchCache::new(AtlasFormat::Rgba16F, 4096);
    assert_eq!(
        cache.acquire(UVec2::new(4097, 10), Tenant::Backdrops.owner(), 0),
        Err(AtlasError::Oversized {
            size: UVec2::new(4097, 10),
            edge: 4096
        })
    );
    let id = cache
        .acquire(UVec2::new(3100, 10), Tenant::Backdrops.owner(), 0)
        .unwrap();
    assert_eq!(cache.size(id), Some(UVec2::new(4096, 16)));
}
