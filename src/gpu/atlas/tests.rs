//! The pool against a reference model over a long seeded run of inserts, releases, draws, pins and
//! frames.

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::atlas::page_list::{MAX_PAGES, PageListConfig};
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::atlas::{AtlasPool, error::AtlasError};
use crate::gpu::resource::expiry_window::EXPIRY_FRAMES;
use glam::{U16Vec2, UVec2};
use std::collections::BTreeMap;

/// A xorshift64 stream, so a failing run replays from its seed.
#[derive(Debug)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// What the model knows of a live item: its handle, where the pool put it, its pins and the
/// frame it was last drawn in.
#[derive(Clone, Copy, Debug)]
struct Held {
    handle: AtlasHandle,
    placement: Placement,
    pins: u32,
    last_use: u64,
}

/// Over four thousand random steps on `R8` lists small enough to grow, evict and fill, the pool
/// agrees with a model after each: every live item sits where it was put, inside its page, apart
/// from every other on that page; each page's live texels are its items'; the bytes are the open
/// pages'; and every evicted item was neither pinned nor drawn this frame or the last, and reads
/// as stale.
#[test]
fn the_pool_agrees_with_a_model_over_a_seeded_run() {
    let config = PageListConfig {
        first_edge: 32,
        page_edge: 64,
        soft_bytes: 2 * 64 * 64,
    };
    let mut pool = AtlasPool::with_configs([config; 3], 16384);
    // By slot and generation, so a walk over it runs in one order every run.
    let mut model: BTreeMap<(u32, u32), Held> = BTreeMap::new();
    let mut drained = Vec::new();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let tenants = [
        Tenant::Glyphs,
        Tenant::Icons,
        Tenant::CutoutTables,
        Tenant::Backdrops,
    ];
    let mut refused = 0;
    let mut evicted = 0;
    for step in 0..4000 {
        let live: Vec<AtlasHandle> = model.values().map(|held| held.handle).collect();
        let pick = |rng: &mut Rng| live[rng.below(live.len() as u64) as usize];
        match rng.below(10) {
            0..=4 => {
                let size = U16Vec2::new(1 + rng.below(40) as u16, 1 + rng.below(40) as u16);
                let tenant = tenants[rng.below(tenants.len() as u64) as usize];
                match pool.allocate(AtlasFormat::R8, tenant, size) {
                    Ok(handle) => {
                        let placement = pool.placement(handle).expect("a new item is live");
                        assert_eq!(placement.size, size, "step {step}");
                        model.insert(
                            key(handle),
                            Held {
                                handle,
                                placement,
                                pins: 0,
                                last_use: pool.frame(),
                            },
                        );
                    }
                    Err(AtlasError::Full) => refused += 1,
                    Err(error) => panic!("step {step}: {error}"),
                }
            }
            5 if !live.is_empty() => {
                let handle = pick(&mut rng);
                if model[&key(handle)].pins == 0 {
                    pool.release(handle);
                    model.remove(&key(handle));
                    assert_eq!(pool.placement(handle), None, "step {step}");
                }
            }
            6 if !live.is_empty() => {
                let handle = pick(&mut rng);
                assert!(pool.stamp(handle).is_some(), "step {step}");
                model.get_mut(&key(handle)).unwrap().last_use = pool.frame();
            }
            7 if !live.is_empty() => {
                let handle = pick(&mut rng);
                let held = model.get_mut(&key(handle)).unwrap();
                if held.pins > 0 && rng.below(2) == 0 {
                    pool.unpin(handle);
                    held.pins -= 1;
                } else if held.pins < 2 {
                    pool.pin(handle);
                    held.pins += 1;
                }
            }
            8 | 9 => pool.advance_to(pool.frame() + 1),
            _ => {}
        }
        for tenant in tenants {
            drained.clear();
            drained.extend(pool.drain_evicted(tenant));
            for &handle in &drained {
                let held = model
                    .remove(&key(handle))
                    .expect("an evicted item was live");
                assert_eq!(held.pins, 0, "step {step}: a pinned item evicted");
                assert!(
                    held.last_use + 1 < pool.frame(),
                    "step {step}: an item drawn at {} evicted at {}",
                    held.last_use,
                    pool.frame()
                );
                assert_eq!(pool.placement(handle), None, "step {step}");
                evicted += 1;
            }
        }
        check(&pool, &model, step);
    }
    assert!(evicted > 100, "the run evicts, {evicted} items");
    assert!(refused > 0, "the run reaches the hard cap");
}

/// The model's key of `handle`.
const fn key(handle: AtlasHandle) -> (u32, u32) {
    (handle.idx, handle.generation)
}

/// Every invariant the model holds the pool to after `step`.
fn check(pool: &AtlasPool, model: &BTreeMap<(u32, u32), Held>, step: usize) {
    let list = pool.list(AtlasFormat::R8);
    let mut by_page: [Vec<Placement>; MAX_PAGES] = Default::default();
    for held in model.values() {
        assert_eq!(
            pool.placement(held.handle),
            Some(held.placement),
            "step {step}"
        );
        by_page[held.placement.page as usize].push(held.placement);
    }
    let mut bytes = 0;
    for (idx, placements) in by_page.iter().enumerate() {
        let Some(page) = list.page(idx) else {
            assert!(placements.is_empty(), "step {step}: items on a closed page");
            continue;
        };
        let edge = page.edge();
        bytes += u64::from(edge).pow(2);
        let texels: u64 = placements.iter().map(|placement| placement.texels()).sum();
        assert_eq!(page.live_texels(), texels, "step {step}: page {idx}");
        for (at, a) in placements.iter().enumerate() {
            let a_max = a.origin.as_uvec2() + a.size.as_uvec2();
            assert!(a_max.max_element() <= edge, "step {step}: past the page");
            for b in &placements[at + 1..] {
                let b_max = b.origin.as_uvec2() + b.size.as_uvec2();
                let apart = a_max.x <= u32::from(b.origin.x)
                    || b_max.x <= u32::from(a.origin.x)
                    || a_max.y <= u32::from(b.origin.y)
                    || b_max.y <= u32::from(a.origin.y);
                assert!(apart, "step {step}: {a:?} overlaps {b:?}");
            }
        }
    }
    assert_eq!(list.bytes(), bytes, "step {step}");
}

/// The pool's lists take the proposal's starting constants by format: `R8` from 256 to 2048 pages
/// under 8 MiB, `Rgba8` from 256 to 1024 under 8 MiB, `Rgba16F` from 256 to 1024 under 32 MiB; a
/// device of 1024 holds the `R8` page edge to 1024.
#[test]
fn the_pool_takes_the_starting_constants() {
    let pool = AtlasPool::new(16384);
    let config = |format| pool.list(format).config();
    assert_eq!(
        config(AtlasFormat::R8),
        PageListConfig {
            first_edge: 256,
            page_edge: 2048,
            soft_bytes: 8 << 20
        }
    );
    assert_eq!(
        config(AtlasFormat::Rgba8),
        PageListConfig {
            first_edge: 256,
            page_edge: 1024,
            soft_bytes: 8 << 20
        }
    );
    assert_eq!(
        config(AtlasFormat::Rgba16F),
        PageListConfig {
            first_edge: 256,
            page_edge: 1024,
            soft_bytes: 32 << 20
        }
    );
    assert_eq!(
        AtlasPool::new(1024)
            .list(AtlasFormat::R8)
            .config()
            .page_edge,
        1024
    );
}

/// A full-window glass result takes a texture of its own on the ladder, 1936 × 1088 to
/// 2048 × 1536, `2048 · 1536 · 8 = 25165824` bytes of `Rgba16F`; released at frame 3, it idles
/// through frame `3 + EXPIRY_FRAMES - 1` and goes at `3 + EXPIRY_FRAMES` as the pool's frames move.
#[test]
fn a_scratch_texture_idles_then_expires_with_the_frames() {
    let mut pool = AtlasPool::new(16384);
    let id = pool
        .acquire_scratch(AtlasFormat::Rgba16F, UVec2::new(1936, 1088))
        .unwrap();
    assert_eq!(
        pool.scratch_size(AtlasFormat::Rgba16F, id),
        Some(UVec2::new(2048, 1536))
    );
    let bytes = 2048 * 1536 * 8;
    assert_eq!(pool.list(AtlasFormat::Rgba16F).bytes(), bytes);
    pool.advance_to(3);
    pool.release_scratch(AtlasFormat::Rgba16F, id);
    assert_eq!(pool.scratch_size(AtlasFormat::Rgba16F, id), None);
    pool.advance_to(3 + EXPIRY_FRAMES - 1);
    assert_eq!(
        pool.list(AtlasFormat::Rgba16F).bytes(),
        bytes,
        "a frame short"
    );
    pool.advance_to(3 + EXPIRY_FRAMES);
    assert_eq!(pool.list(AtlasFormat::Rgba16F).bytes(), 0);
}
