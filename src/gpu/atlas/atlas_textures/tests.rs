//! The GPU half following the CPU half: a page made, grown with its texels, closed and opened
//! again empty; scratch made and dropped; the bytes on both halves alike.

use super::*;
use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::atlas::PoolConfig;
use crate::gpu::atlas::page_list::{MoveBudget, PageListConfig};
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::test_gpu::{TestGpu, headless_test_gpu};
use glam::U16Vec2;
use std::ptr;

/// A pool whose lists open at 32 and double to 64.
fn pool() -> AtlasPool {
    AtlasPool::uniform(32, 64, 1 << 20)
}

/// A pool whose lists open at `first_edge`, double to `page_edge` and evict past `soft_bytes`,
/// moving within the budget of a device whose passes move `traffic`.
fn moving_pool(
    first_edge: u32,
    page_edge: u32,
    soft_bytes: u64,
    traffic: PassTraffic,
) -> AtlasPool {
    budget_pool(first_edge, page_edge, soft_bytes, MoveBudget::of(traffic))
}

/// A pool as [`moving_pool`] makes it, whose display frames move within `move_budget`.
fn budget_pool(
    first_edge: u32,
    page_edge: u32,
    soft_bytes: u64,
    move_budget: MoveBudget,
) -> AtlasPool {
    AtlasPool::new(PoolConfig {
        lists: PerFormat::from_fn(|_| PageListConfig {
            first_edge,
            page_edge,
            soft_bytes,
        }),
        device_edge: 16384,
        move_budget,
    })
}

/// Commit `pool` into `textures` on an encoder of its own, submitted and waited for.
fn commit(gpu: &TestGpu, textures: &mut AtlasTextures, pool: &mut AtlasPool) {
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    textures.commit(gpu, &mut encoder, pool);
    gpu.queue.submit([encoder.finish()]);
    gpu.wait();
}

/// Write `texels`, one byte each, over the whole of `texture`, an `R8Unorm` of `edge`².
fn fill(gpu: &TestGpu, texture: &wgpu::Texture, edge: u32, texels: &[u8]) {
    gpu.queue.write_texture(
        texture.as_image_copy(),
        texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(edge),
            rows_per_image: None,
        },
        texture.size(),
    );
}

/// The first page's texture opens at 32; a grow of the page to 64 makes a 64² texture whose
/// top-left 32² holds the old texels, byte for byte, the rest zero; each change moves the
/// generation once, and a commit with no change moves nothing. The bytes on the GPU are the CPU
/// half's: 32² then 64².
#[test]
fn a_grown_page_keeps_its_texels() {
    let gpu = headless_test_gpu();
    let mut pool = pool();
    let mut textures = AtlasTextures::new(&gpu);
    pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(16))
        .unwrap();
    commit(&gpu, &mut textures, &mut pool);
    assert_eq!(textures.generation(AtlasFormat::R8), 1);
    assert_eq!(textures.bytes(AtlasFormat::R8), 32 * 32);
    let pattern: Vec<u8> = (0..32 * 32u32).map(|i| (i % 251) as u8).collect();
    fill(
        &gpu,
        textures.page_texture(AtlasFormat::R8, 0).unwrap(),
        32,
        &pattern,
    );

    pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(32))
        .unwrap();
    assert_eq!(pool.list(AtlasFormat::R8).page(0).unwrap().edge(), 64);
    commit(&gpu, &mut textures, &mut pool);
    assert_eq!(textures.generation(AtlasFormat::R8), 2);
    assert_eq!(
        textures.bytes(AtlasFormat::R8),
        pool.list(AtlasFormat::R8).bytes()
    );
    let read = gpu.read_texture(textures.page_texture(AtlasFormat::R8, 0).unwrap());
    for y in 0..64usize {
        for x in 0..64usize {
            let want = if x < 32 && y < 32 {
                pattern[y * 32 + x]
            } else {
                0
            };
            assert_eq!(read[y * 64 + x], want, "texel ({x}, {y})");
        }
    }
    commit(&gpu, &mut textures, &mut pool);
    assert_eq!(textures.generation(AtlasFormat::R8), 2, "nothing changed");
}

/// A drain's move lands its item's texels at its new place byte for byte at the next commit, by
/// copy and by draw alike, though the page it left closed in the same decay: in each format's list
/// four 16² items fill a 32² page and a fifth opens a second; with two of the first page's gone,
/// the decay drains the second into the first's free (16, 0) and closes it. One commit moves each
/// list's second page's top-left 16² there, three runs of one submission, keeps every other texel
/// of the first page, and drops the second page's texture, one change of view a list. Every byte
/// differs from its neighbors and no half-float is a NaN, whose bits a draw may not keep.
#[test]
fn a_move_lands_its_texels_from_the_page_it_left() {
    let gpu = headless_test_gpu();
    for traffic in [PassTraffic::WholeTarget, PassTraffic::Drawn] {
        moves_land(&gpu, traffic);
    }
}

/// [`a_move_lands_its_texels_from_the_page_it_left`], moving by `route`.
fn moves_land(gpu: &TestGpu, traffic: PassTraffic) {
    let mut pool = moving_pool(32, 32, 1 << 20, traffic);
    let mut textures = AtlasTextures::with_traffic(gpu, traffic);
    let insert = |pool: &mut AtlasPool, format| {
        pool.allocate(format, Tenant::Glyphs, UVec2::splat(16))
            .unwrap()
            .handle
    };
    let items = AtlasFormat::ALL.map(|format| {
        let first: Vec<_> = (0..4).map(|_| insert(&mut pool, format)).collect();
        let fifth = insert(&mut pool, format);
        (first, fifth)
    });
    commit(gpu, &mut textures, &mut pool);
    let row = |format: AtlasFormat| 32 * format.texel_bytes() as usize;
    // Every byte below 0x7c keeps each half's exponent off its all-ones.
    let patterns = AtlasFormat::ALL.map(|format| {
        [0, 1].map(|page| {
            (0..32 * row(format))
                .map(|i| ((i + page * 13 + format as usize * 29) % 0x7b) as u8)
                .collect::<Vec<u8>>()
        })
    });
    for format in AtlasFormat::ALL {
        for (page, pattern) in patterns[format as usize].iter().enumerate() {
            let texture = textures.page_texture(format, page).unwrap();
            gpu.queue.write_texture(
                texture.as_image_copy(),
                pattern,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row(format) as u32),
                    rows_per_image: None,
                },
                texture.size(),
            );
        }
        let (first, _) = &items[format as usize];
        pool.release(first[1]);
        pool.release(first[2]);
    }
    pool.advance_to(1);
    pool.end_submit();
    let generations = AtlasFormat::ALL.map(|format| textures.generation(format));
    commit(gpu, &mut textures, &mut pool);
    for format in AtlasFormat::ALL {
        let (_, fifth) = items[format as usize];
        assert_eq!(pool.placement(fifth).unwrap().page, 0);
        assert!(pool.list(format).page(1).is_none());
        assert_eq!(
            textures.generation(format),
            generations[format as usize] + 1
        );
        assert!(textures.page_texture(format, 1).is_none());
        let read = gpu.read_texture(textures.page_texture(format, 0).unwrap());
        let texel = format.texel_bytes() as usize;
        let row = row(format);
        let [kept, moved] = &patterns[format as usize];
        for y in 0..32usize {
            for x in 0..32usize {
                let at = y * row + x * texel;
                let want = if (16..32).contains(&x) && y < 16 {
                    &moved[y * row + (x - 16) * texel..][..texel]
                } else {
                    &kept[at..][..texel]
                };
                assert_eq!(
                    &read[at..at + texel],
                    want,
                    "{format:?} by {traffic:?}: texel ({x}, {y})"
                );
            }
        }
    }
}

/// A commit lands its moves in the order they were made, by copy and by draw alike, as a later
/// one may land where an earlier one's item lay. Sixteen 8² items fill a 32² page and two more, `x`
/// and `y`, open a second; four of the first page's go, and a decay under a budget of one move
/// drains `x` to the first freed place, (0, 0). A purge before the commit evicts `x`, the one
/// item no one pins, and goes on with the drain, which lands `y` on the same place. The commit
/// holds both moves, and the place reads `y`'s texels, every other texel of the page its own.
#[test]
fn a_commit_lands_its_moves_in_the_order_they_were_made() {
    let gpu = headless_test_gpu();
    let one = MoveBudget {
        texels: u64::MAX,
        moves: 1,
    };
    for traffic in [PassTraffic::WholeTarget, PassTraffic::Drawn] {
        let mut pool = budget_pool(32, 32, 1 << 20, one);
        let mut textures = AtlasTextures::with_traffic(&gpu, traffic);
        let insert = |pool: &mut AtlasPool| {
            pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(8))
                .unwrap()
                .handle
        };
        let first: Vec<_> = (0..16).map(|_| insert(&mut pool)).collect();
        let x = insert(&mut pool);
        let y = insert(&mut pool);
        let origin = |pool: &AtlasPool, handle| {
            let placement = pool.placement(handle).unwrap();
            (placement.page, placement.origin)
        };
        assert_eq!(origin(&pool, x), (1, U16Vec2::ZERO));
        assert_eq!(origin(&pool, y), (1, U16Vec2::new(8, 0)));
        commit(&gpu, &mut textures, &mut pool);
        let page = |idx| textures.page_texture(AtlasFormat::R8, idx).unwrap();
        fill(&gpu, page(0), 32, &[0x33; 32 * 32]);
        let second: Vec<u8> = (0..32 * 32)
            .map(|i| match (i % 32, i / 32) {
                (0..8, 0..8) => 0x11,
                (8..16, 0..8) => 0x22,
                _ => 0,
            })
            .collect();
        fill(&gpu, page(1), 32, &second);

        for &handle in &first[..4] {
            pool.release(handle);
        }
        for &handle in first[4..].iter().chain([&y]) {
            pool.pin(handle);
        }
        pool.advance_to(1);
        pool.end_submit();
        assert_eq!(origin(&pool, x), (0, U16Vec2::ZERO));
        pool.purge();
        assert_eq!(pool.placement(x), None, "evicted, its move waiting");
        assert_eq!(origin(&pool, y), (0, U16Vec2::ZERO));
        assert_eq!(pool.list(AtlasFormat::R8).moves().len(), 2);
        commit(&gpu, &mut textures, &mut pool);
        assert!(pool.list(AtlasFormat::R8).moves().is_empty());
        let read = gpu.read_texture(textures.page_texture(AtlasFormat::R8, 0).unwrap());
        for (i, &texel) in read.iter().enumerate() {
            let want = if i % 32 < 8 && i / 32 < 8 { 0x22 } else { 0x33 };
            assert_eq!(
                texel,
                want,
                "by {traffic:?}: texel ({}, {})",
                i % 32,
                i / 32
            );
        }
    }
}

/// The `R8` texel page `page` of `edge`² holds in [`page_pattern`], at `(x, y)`.
fn page_texel(page: u32, x: u32, y: u32) -> u8 {
    ((x * 7 + y * 13 + page * 29) % 251) as u8
}

/// Every texel of an `R8` page `page` of `edge`², row by row, as [`page_texel`] gives it.
fn page_pattern(page: u32, edge: u32) -> Vec<u8> {
    (0..edge * edge)
        .map(|i| page_texel(page, i % edge, i / edge))
        .collect()
}

/// A drain whose items land on two pages moves each from its source by copy and by draw alike, a
/// run a destination by draw. On 64² pages of 16² items the first holds 15, the second 8 and the
/// third 2: the third, the emptiest, drains, its first item into the first page's hole at (16, 16),
/// its second into the second page's first free place at (0, 0). Each destination keeps every other
/// texel, and the third page closes.
#[test]
fn a_drain_lands_on_two_pages() {
    let gpu = headless_test_gpu();
    for traffic in [PassTraffic::WholeTarget, PassTraffic::Drawn] {
        let mut pool = moving_pool(64, 64, 1 << 20, traffic);
        let mut textures = AtlasTextures::with_traffic(&gpu, traffic);
        let items: Vec<_> = (0..34)
            .map(|_| {
                pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(16))
                    .unwrap()
                    .handle
            })
            .collect();
        commit(&gpu, &mut textures, &mut pool);
        for page in 0..3 {
            fill(
                &gpu,
                textures
                    .page_texture(AtlasFormat::R8, page as usize)
                    .unwrap(),
                64,
                &page_pattern(page, 64),
            );
        }
        pool.release(items[5]);
        for &handle in &items[16..24] {
            pool.release(handle);
        }
        pool.advance_to(1);
        pool.end_submit();
        let moved: Vec<_> = pool
            .list(AtlasFormat::R8)
            .moves()
            .iter()
            .map(|m| (m.from.page, m.from.origin, m.to.page, m.to.origin))
            .collect();
        assert_eq!(
            moved,
            [
                (2, U16Vec2::new(0, 0), 0, U16Vec2::new(16, 16)),
                (2, U16Vec2::new(16, 0), 1, U16Vec2::new(0, 0)),
            ]
        );
        commit(&gpu, &mut textures, &mut pool);
        assert!(textures.page_texture(AtlasFormat::R8, 2).is_none());
        for (page, at, from_x) in [(0, UVec2::new(16, 16), 0), (1, UVec2::ZERO, 16)] {
            let read = gpu.read_texture(textures.page_texture(AtlasFormat::R8, page).unwrap());
            for y in 0..64 {
                for x in 0..64 {
                    let inside = (at.x..at.x + 16).contains(&x) && (at.y..at.y + 16).contains(&y);
                    let want = if inside {
                        page_texel(2, from_x + x - at.x, y - at.y)
                    } else {
                        page_texel(page as u32, x, y)
                    };
                    assert_eq!(
                        read[(y * 64 + x) as usize],
                        want,
                        "{traffic:?}: page {page} texel ({x}, {y})"
                    );
                }
            }
        }
    }
}

/// A move's source is the page it left though a new page opened in its slot before the commit:
/// the commit reads the closed page's texture by its opening, and the new page starts empty. On 32²
/// pages four 16² items fill the first and a fifth opens the second; two of the first page's go,
/// and the decay drains the second into the first's (16, 0), closing it. A 32² item, which the
/// first page has no room for, opens a page in the second slot before the commit.
#[test]
fn a_move_reads_its_page_though_its_slot_opened_again() {
    let gpu = headless_test_gpu();
    for traffic in [PassTraffic::WholeTarget, PassTraffic::Drawn] {
        let mut pool = moving_pool(32, 32, 1 << 20, traffic);
        let mut textures = AtlasTextures::with_traffic(&gpu, traffic);
        let items: Vec<_> = (0..5)
            .map(|_| {
                pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(16))
                    .unwrap()
                    .handle
            })
            .collect();
        commit(&gpu, &mut textures, &mut pool);
        for page in 0..2 {
            fill(
                &gpu,
                textures
                    .page_texture(AtlasFormat::R8, page as usize)
                    .unwrap(),
                32,
                &page_pattern(page, 32),
            );
        }
        pool.release(items[1]);
        pool.release(items[2]);
        pool.advance_to(1);
        pool.end_submit();
        assert!(pool.list(AtlasFormat::R8).page(1).is_none(), "closed");
        let wide = pool
            .allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(32))
            .unwrap()
            .handle;
        assert_eq!(pool.placement(wide).unwrap().page, 1, "opened again");
        commit(&gpu, &mut textures, &mut pool);
        let first = gpu.read_texture(textures.page_texture(AtlasFormat::R8, 0).unwrap());
        for y in 0..32 {
            for x in 0..32 {
                let want = if (16..32).contains(&x) && y < 16 {
                    page_texel(1, x - 16, y)
                } else {
                    page_texel(0, x, y)
                };
                assert_eq!(
                    first[(y * 32 + x) as usize],
                    want,
                    "{traffic:?}: ({x}, {y})"
                );
            }
        }
        let again = gpu.read_texture(textures.page_texture(AtlasFormat::R8, 1).unwrap());
        assert!(again.iter().all(|&texel| texel == 0), "{traffic:?}: empty");
    }
}

/// A closed page drops its texture and its view slot binds the stand-in, where an open page binds
/// its own; a page opened in that
/// slot again is another page, so its texture starts empty rather than with the closed page's
/// texels.
#[test]
fn a_page_opened_again_in_its_slot_starts_empty() {
    let gpu = headless_test_gpu();
    let mut pool = pool();
    let mut textures = AtlasTextures::new(&gpu);
    let side = UVec2::splat(64);
    pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(40))
        .unwrap();
    let wide = pool
        .allocate(AtlasFormat::R8, Tenant::Glyphs, side)
        .unwrap()
        .handle;
    assert_eq!(pool.placement(wide).unwrap().page, 1);
    commit(&gpu, &mut textures, &mut pool);
    fill(
        &gpu,
        textures.page_texture(AtlasFormat::R8, 1).unwrap(),
        64,
        &[0xaa; 64 * 64],
    );

    pool.release(wide);
    pool.close_page(AtlasFormat::R8, 1);
    let generation = textures.generation(AtlasFormat::R8);
    commit(&gpu, &mut textures, &mut pool);
    assert!(textures.page_texture(AtlasFormat::R8, 1).is_none());
    assert_eq!(textures.generation(AtlasFormat::R8), generation + 1);
    let list = &textures.lists[AtlasFormat::R8];
    let views = textures.views(AtlasFormat::R8);
    let open = &list.pages[0].as_ref().unwrap().bound.view;
    let absent = &list.absent.view;
    assert!(ptr::eq(views[0], open), "an open page binds its own view");
    assert!(ptr::eq(views[1], absent), "a closed one the stand-in");

    let again = pool
        .allocate(AtlasFormat::R8, Tenant::Glyphs, side)
        .unwrap()
        .handle;
    assert_eq!(pool.placement(again).unwrap().page, 1);
    commit(&gpu, &mut textures, &mut pool);
    let read = gpu.read_texture(textures.page_texture(AtlasFormat::R8, 1).unwrap());
    assert!(
        read.iter().all(|&texel| texel == 0),
        "the closed page's texels stay gone"
    );
}

/// A scratch slot makes its texture at its rung, 100² to 128², and drops it once it expires, each
/// a change of view that moves the generation once; the GPU's bytes follow the CPU half's
/// throughout.
#[test]
fn a_scratch_texture_follows_its_slot() {
    let gpu = headless_test_gpu();
    let mut pool = pool();
    let mut textures = AtlasTextures::new(&gpu);
    let id = pool
        .acquire_scratch(AtlasFormat::R8, UVec2::splat(100))
        .unwrap();
    commit(&gpu, &mut textures, &mut pool);
    assert!(textures.scratch_view(id).is_some());
    assert_eq!(textures.bytes(AtlasFormat::R8), 128 * 128);
    assert_eq!(textures.generation(AtlasFormat::R8), 1);
    pool.release_scratch(id);
    commit(&gpu, &mut textures, &mut pool);
    assert_eq!(
        textures.generation(AtlasFormat::R8),
        1,
        "an idle texture stays"
    );
    pool.advance_to(EXPIRY_FRAMES);
    pool.end_submit();
    commit(&gpu, &mut textures, &mut pool);
    assert!(textures.scratch_view(id).is_none());
    assert_eq!(textures.bytes(AtlasFormat::R8), 0);
    assert_eq!(textures.generation(AtlasFormat::R8), 2);
}

/// Textures made again by their constructor, as on a new device, take every open page of every
/// list at its edge at the first commit, empty for the tenants to fill again.
#[test]
fn new_textures_take_every_open_page_at_the_first_commit() {
    let gpu = headless_test_gpu();
    let mut pool = pool();
    for format in AtlasFormat::ALL {
        pool.allocate(format, Tenant::Glyphs, UVec2::splat(40))
            .unwrap();
        pool.allocate(format, Tenant::Glyphs, UVec2::splat(64))
            .unwrap();
    }
    let mut textures = AtlasTextures::new(&gpu);
    commit(&gpu, &mut textures, &mut pool);
    for format in AtlasFormat::ALL {
        assert_eq!(
            textures.bytes(format),
            pool.list(format).bytes(),
            "{format:?}"
        );
        assert_eq!(
            [0, 1].map(|idx| textures.page_texture(format, idx).map(wgpu::Texture::width)),
            [Some(64), Some(64)],
            "{format:?}"
        );
    }
}
