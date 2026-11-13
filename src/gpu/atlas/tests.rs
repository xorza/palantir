//! The pool against a reference model over a long seeded run of inserts, releases, draws, pins and
//! frames.

use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{Allocation, AtlasHandle, Placement};
use crate::gpu::atlas::page_list::{MAX_PAGES, Page, PageListConfig};
use crate::gpu::atlas::scratch_cache::ScratchId;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::atlas::{AtlasPool, error::AtlasError};
use crate::internals::seeded_rng::SeededRng;
use glam::UVec2;
use std::collections::BTreeMap;

/// What the model knows of a live item: its handle, where the pool put it, its pins and the
/// frame it was last drawn in.
#[derive(Clone, Copy, Debug)]
struct Held {
    handle: AtlasHandle,
    placement: Placement,
    pins: u32,
    last_use: u64,
}

/// Over four thousand random steps on `R8` lists small enough to grow, evict, fill and decay, the
/// pool agrees with a model after each: every live item sits where it was put or its last move
/// took it, inside its page, apart from every other on that page; each page's live texels are its
/// items'; the bytes are the open pages'; every evicted item was neither pinned nor drawn this
/// frame or the last, and reads as stale; and no frame's moves land on a page they leave.
#[test]
fn the_pool_agrees_with_a_model_over_a_seeded_run() {
    let mut pool = AtlasPool::uniform(32, 64, 2 * 64 * 64);
    // By slot and generation, so a walk over it runs in one order every run.
    let mut model: BTreeMap<(u32, u32), Held> = BTreeMap::new();
    let mut drained = Vec::new();
    let mut rng = SeededRng::new(0x9e37_79b9_7f4a_7c15);
    let tenants = [
        Tenant::Glyphs,
        Tenant::Icons,
        Tenant::CutoutTables,
        Tenant::Backdrops,
    ];
    let mut refused = 0;
    let mut evicted = 0;
    let mut moved = 0;
    for step in 0..4000 {
        let live: Vec<AtlasHandle> = model.values().map(|held| held.handle).collect();
        let pick = |rng: &mut SeededRng| live[rng.below(live.len() as u64) as usize];
        match rng.below(10) {
            0..=4 => {
                let size = UVec2::new(1 + rng.below(40) as u32, 1 + rng.below(40) as u32);
                let tenant = tenants[rng.below(tenants.len() as u64) as usize];
                match pool.allocate(AtlasFormat::R8, tenant, size) {
                    Ok(Allocation { handle, placement }) => {
                        assert_eq!(pool.placement(handle), Some(placement), "step {step}");
                        assert_eq!(placement.size.as_uvec2(), size, "step {step}");
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
                    held.last_use = pool.frame();
                } else if held.pins < 2 {
                    pool.pin(handle);
                    held.pins += 1;
                }
            }
            8 | 9 => {
                pool.advance_to(pool.frame() + 1);
                pool.end_submit();
                let moves = pool.list(AtlasFormat::R8).moves().to_vec();
                pool.forget_moves();
                for m in &moves {
                    assert!(
                        moves.iter().all(|other| other.to.page != m.from.page),
                        "step {step}: page {} a source and a destination",
                        m.from.page
                    );
                    let held = model
                        .values_mut()
                        .find(|held| held.placement == m.from)
                        .expect("a moved item was live");
                    held.placement = m.to;
                    moved += 1;
                }
            }
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
    assert!(moved > 0, "the run drains");
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

/// A device's pool takes the starting constants by format: `R8` from 256 to 2048 pages under
/// 8 MiB, `Rgba8` from 256 to 1024 under 8 MiB, `Rgba16F` from 256 to 1024 under 32 MiB; a device
/// of 1024 holds the `R8` page edge to 1024.
#[test]
fn the_pool_takes_the_starting_constants() {
    let pool = AtlasPool::of_edge(16384);
    let config = |format| pool.list(format).config();
    let want = |first_edge, page_edge, soft_bytes| PageListConfig {
        first_edge,
        page_edge,
        soft_bytes,
    };
    assert_eq!(config(AtlasFormat::R8), want(256, 2048, 8 << 20));
    assert_eq!(config(AtlasFormat::Rgba8), want(256, 1024, 8 << 20));
    assert_eq!(config(AtlasFormat::Rgba16F), want(256, 1024, 32 << 20));
    assert_eq!(
        AtlasPool::of_edge(1024)
            .list(AtlasFormat::R8)
            .config()
            .page_edge,
        1024
    );
}

/// A full-window glass result takes a texture of its own on the ladder, 1936 × 1088 to
/// 2048 × 1536, `2048 · 1536 · 8 = 25165824` bytes of `Rgba16F`; released at frame 3, it idles
/// through frame `3 + EXPIRY_FRAMES - 1` and goes at the decay of `3 + EXPIRY_FRAMES`.
#[test]
fn a_scratch_texture_idles_then_expires_with_the_frames() {
    let mut pool = AtlasPool::of_edge(16384);
    let id = pool
        .acquire_scratch(
            AtlasFormat::Rgba16F,
            Tenant::Backdrops,
            UVec2::new(1936, 1088),
        )
        .unwrap();
    assert_eq!(pool.scratch_size(id), Some(UVec2::new(2048, 1536)));
    let bytes = 2048 * 1536 * 8;
    assert_eq!(pool.list(AtlasFormat::Rgba16F).bytes(), bytes);
    pool.advance_to(3);
    pool.release_scratch(id);
    assert_eq!(pool.scratch_size(id), None);
    pool.advance_to(3 + EXPIRY_FRAMES - 1);
    pool.end_submit();
    assert_eq!(
        pool.list(AtlasFormat::Rgba16F).bytes(),
        bytes,
        "a frame short"
    );
    pool.advance_to(3 + EXPIRY_FRAMES);
    pool.end_submit();
    assert_eq!(pool.list(AtlasFormat::Rgba16F).bytes(), 0);
}

/// A burst of glyphs that opens a second mask page ages out within the bounds until the list is
/// back on one page under its budget, and then ages nothing out. Under the starting constants 5000
/// items of 32² at frame 1 fill the first page at the 2048 page edge, 4096 items, and open a second
/// for the other 904: two pages, their items 5 000 KiB, under the 8 MiB budget, a pressure of 0
/// and an age of 400, so the first go at frame 402, at most 32 a frame. Every eviction falls while two pages are open; once the drain
/// closes the second, the one page of 4 MiB is under the budget and keeps what it holds through
/// frame 1500. The ten a frame draws stay throughout, and no frame moves past a copy device's
/// budget, 16 copies and 512² texels.
#[test]
fn a_burst_past_one_page_decays_within_the_bounds() {
    let mut pool = AtlasPool::of_edge(16384);
    pool.advance_to(1);
    let burst: Vec<AtlasHandle> = (0..5000)
        .map(|_| {
            pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(32))
                .unwrap()
                .handle
        })
        .collect();
    let edges = |pool: &AtlasPool| -> Vec<u32> {
        let list = pool.list(AtlasFormat::R8);
        (0..MAX_PAGES)
            .filter_map(|idx| list.page(idx).map(Page::edge))
            .collect()
    };
    assert_eq!(edges(&pool), [2048, 2048]);
    assert_eq!(pool.list(AtlasFormat::R8).bytes(), 8 << 20);
    let kept = &burst[..10];
    let mut first_eviction = None;
    let mut evicted_total = 0;
    for frame in 2..=1500 {
        pool.advance_to(frame);
        for &handle in kept {
            assert!(
                pool.stamp(handle).is_some(),
                "frame {frame}: a drawn item went"
            );
        }
        let pages_before = edges(&pool).len();
        pool.end_submit();
        let evicted = pool.drain_evicted(Tenant::Glyphs).count();
        assert!(evicted <= 32, "frame {frame}: {evicted} evictions");
        if evicted > 0 {
            assert_eq!(pages_before, 2, "frame {frame}: an eviction on one page");
            first_eviction.get_or_insert(frame);
        }
        evicted_total += evicted;
        let moves = pool.list(AtlasFormat::R8).moves().to_vec();
        pool.forget_moves();
        let texels: u64 = moves.iter().map(|m| m.from.texels()).sum();
        assert!(moves.len() <= 16, "frame {frame}: {} moves", moves.len());
        assert!(texels <= 512 * 512, "frame {frame}: {texels} texels moved");
    }
    assert_eq!(first_eviction, Some(402));
    assert_eq!(edges(&pool), [2048], "the second page drained and closed");
    let live = burst
        .iter()
        .filter(|&&handle| pool.list(AtlasFormat::R8).placement(handle).is_some())
        .count();
    assert_eq!(live, 5000 - evicted_total, "what did not age out stays");
    assert!(live > kept.len(), "idle items stay once on one page");
}

/// A purge, as the system warns of low memory, leaves exactly the items this frame drew and the
/// pinned ones, drops the idle scratch, and drains with no budget. On pages of 32², 16² items fill
/// the first and open a second: at frame 5, one drawn at 5 and one pinned stay of six, one drawn
/// at 4, in the grace a decay keeps, among the rest that go. The second page, its last item gone,
/// closes; the first keeps the two. An idle scratch texture goes; of two held, the one drawn at 5
/// stays, and the other goes to its tenant's queue.
#[test]
fn a_purge_keeps_what_this_frame_drew_and_what_is_pinned() {
    let mut pool = AtlasPool::uniform(32, 32, 1 << 20);
    let items: Vec<AtlasHandle> = (0..6)
        .map(|_| {
            pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(16))
                .unwrap()
                .handle
        })
        .collect();
    assert!(pool.list(AtlasFormat::R8).page(1).is_some());
    let idle = pool
        .acquire_scratch(AtlasFormat::R8, Tenant::Backdrops, UVec2::splat(64))
        .unwrap();
    let held = pool
        .acquire_scratch(AtlasFormat::R8, Tenant::Backdrops, UVec2::splat(64))
        .unwrap();
    let undrawn = pool
        .acquire_scratch(AtlasFormat::R8, Tenant::Backdrops, UVec2::splat(64))
        .unwrap();
    pool.release_scratch(idle);
    pool.advance_to(4);
    pool.stamp(items[1]).unwrap();
    pool.advance_to(5);
    pool.stamp(items[0]).unwrap();
    assert_eq!(pool.stamp_scratch(held), Some(UVec2::splat(64)));
    pool.pin(items[2]);
    pool.purge();
    let live: Vec<bool> = items
        .iter()
        .map(|&handle| pool.placement(handle).is_some())
        .collect();
    assert_eq!(live, [true, false, true, false, false, false]);
    let evicted: Vec<AtlasHandle> = pool.drain_evicted(Tenant::Glyphs).collect();
    assert_eq!(evicted, [items[1], items[3], items[4], items[5]]);
    let list = pool.list(AtlasFormat::R8);
    assert!(list.page(1).is_none(), "the emptied page closed");
    assert_eq!(list.page(0).unwrap().live_texels(), 2 * 16 * 16);
    assert_eq!(list.scratch().texels(), 64 * 64, "the drawn scratch alone");
    assert_eq!(pool.scratch_size(held), Some(UVec2::splat(64)));
    assert_eq!(pool.scratch_size(undrawn), None);
    let taken: Vec<ScratchId> = pool.drain_evicted_scratch(Tenant::Backdrops).collect();
    assert_eq!(taken, [undrawn]);
}

/// A purge whose drain moved items goes on once they are committed, round after round, until the
/// list is on one page at the smallest edge that packs what it keeps. Forty 32² items grow the
/// first page from 32 to 128, fill it, fill a second 128² page and open a third; at frame 5 the
/// first item of each of the first two is drawn, and the purge takes the other 38. The first round
/// closes the emptied third page and drains the first into the second; the second round shrinks the
/// second, its two items an eighth of it, into a 64² page in the first slot; the third finds
/// nothing to do.
#[test]
fn a_purge_goes_on_round_by_round_to_one_page() {
    let mut pool = AtlasPool::uniform(32, 128, 1 << 30);
    let items: Vec<AtlasHandle> = (0..40)
        .map(|_| {
            pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(32))
                .unwrap()
                .handle
        })
        .collect();
    pool.advance_to(5);
    pool.stamp(items[0]).unwrap();
    pool.stamp(items[16]).unwrap();
    let mut rounds = 0;
    while pool.purge() {
        rounds += 1;
        assert!(rounds <= MAX_PAGES, "a purge ends");
        pool.forget_moves();
    }
    assert_eq!(rounds, 2);
    let list = pool.list(AtlasFormat::R8);
    let edges: Vec<(usize, u32)> = (0..MAX_PAGES)
        .filter_map(|idx| list.page(idx).map(|page| (idx, page.edge())))
        .collect();
    assert_eq!(edges, [(0, 64)]);
    assert_eq!(pool.placement(items[0]).unwrap().page, 0);
    assert_eq!(pool.placement(items[16]).unwrap().page, 0);
}

/// The frame's budget goes to the lists round robin, starting a list later each frame, so a
/// draining mask list holds no other's moves. In the mask and the color lists 256 8² items fill a
/// 128² page and 40 open a second; 128 of the first go, so each second page drains, 40 moves. Under
/// a copy device's 16 moves a frame, frame 3 starts at the mask list, which takes all 16; frame 4
/// at the color list; frame 5 at the half-float list, which drains nothing, then the mask list.
#[test]
fn the_move_budget_goes_round_the_lists() {
    let mut pool = AtlasPool::uniform(128, 128, 1 << 30);
    for format in [AtlasFormat::R8, AtlasFormat::Rgba8] {
        let items: Vec<AtlasHandle> = (0..256 + 40)
            .map(|_| {
                pool.allocate(format, Tenant::Glyphs, UVec2::splat(8))
                    .unwrap()
                    .handle
            })
            .collect();
        for &handle in &items[..128] {
            pool.release(handle);
        }
    }
    let mut taken = Vec::new();
    for frame in 3..6 {
        pool.advance_to(frame);
        pool.end_submit();
        let moves = |format| pool.list(format).moves().len();
        taken.push((moves(AtlasFormat::R8), moves(AtlasFormat::Rgba8)));
        pool.forget_moves();
    }
    assert_eq!(taken, [(16, 0), (0, 16), (16, 0)]);
}

/// A pool a build moved ahead stays there, its rows with it: frame 2, then a submit of frame 1,
/// leaves both on 2; frame 3 moves both.
#[test]
fn a_pool_moved_ahead_stays_ahead() {
    let mut pool = AtlasPool::of_edge(16384);
    pool.advance_to(2);
    pool.advance_to(1);
    assert_eq!((pool.frame(), pool.rows().frame()), (2, 2));
    pool.advance_to(3);
    assert_eq!((pool.frame(), pool.rows().frame()), (3, 3));
}
