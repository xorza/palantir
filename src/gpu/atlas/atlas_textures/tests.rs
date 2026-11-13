//! The GPU half following the CPU half: a page made, grown with its texels, closed and opened
//! again empty; scratch made and dropped; the bytes on both halves alike.

use super::*;
use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::atlas::page_list::PageListConfig;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::test_gpu::{TestGpu, headless_test_gpu};
use glam::U16Vec2;
use std::ptr;

/// A pool whose lists open at 32 and double to 64.
fn pool() -> AtlasPool {
    let config = PageListConfig {
        first_edge: 32,
        page_edge: 64,
        soft_bytes: 1 << 20,
    };
    AtlasPool::with_configs([config; 3], 16384)
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
    pool.allocate(AtlasFormat::R8, Tenant::Glyphs, U16Vec2::splat(16))
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

    pool.allocate(AtlasFormat::R8, Tenant::Glyphs, U16Vec2::splat(32))
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
    for route in [MoveRoute::Copy, MoveRoute::Draw] {
        moves_land(&gpu, route);
    }
}

/// [`a_move_lands_its_texels_from_the_page_it_left`], moving by `route`.
fn moves_land(gpu: &TestGpu, route: MoveRoute) {
    let config = PageListConfig {
        first_edge: 32,
        page_edge: 32,
        soft_bytes: 1 << 20,
    };
    let mut pool = AtlasPool::with_configs([config; 3], 16384);
    let mut textures = AtlasTextures::with_route(gpu, route);
    let insert = |pool: &mut AtlasPool, format| {
        pool.allocate(format, Tenant::Glyphs, U16Vec2::splat(16))
            .unwrap()
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
    pool.end_submit(route.budget());
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
                    "{format:?} by {route:?}: texel ({x}, {y})"
                );
            }
        }
    }
}

/// A device whose passes move their whole target moves by copy, with a frame's 16 copies; any
/// other by draw, with no count of moves, both within 512² texels.
#[test]
fn the_route_follows_what_a_pass_moves() {
    assert_eq!(MoveRoute::of(PassTraffic::WholeTarget), MoveRoute::Copy);
    assert_eq!(MoveRoute::of(PassTraffic::Drawn), MoveRoute::Draw);
    assert_eq!(MoveRoute::Copy.budget().moves, 16);
    assert_eq!(MoveRoute::Draw.budget().moves, u32::MAX);
    for route in [MoveRoute::Copy, MoveRoute::Draw] {
        assert_eq!(route.budget().texels, 512 * 512);
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
    let side = U16Vec2::splat(64);
    pool.allocate(AtlasFormat::R8, Tenant::Glyphs, U16Vec2::splat(40))
        .unwrap();
    let wide = pool
        .allocate(AtlasFormat::R8, Tenant::Glyphs, side)
        .unwrap();
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
    let list = &textures.lists[AtlasFormat::R8 as usize];
    let views = textures.views(AtlasFormat::R8);
    let open = &list.pages[0].as_ref().unwrap().bound.view;
    let absent = &list.absent.view;
    assert!(ptr::eq(views[0], open), "an open page binds its own view");
    assert!(ptr::eq(views[1], absent), "a closed one the stand-in");

    let again = pool
        .allocate(AtlasFormat::R8, Tenant::Glyphs, side)
        .unwrap();
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
    assert!(textures.scratch_view(AtlasFormat::R8, id.idx).is_some());
    assert_eq!(textures.bytes(AtlasFormat::R8), 128 * 128);
    assert_eq!(textures.generation(AtlasFormat::R8), 1);
    pool.release_scratch(AtlasFormat::R8, id);
    commit(&gpu, &mut textures, &mut pool);
    assert_eq!(
        textures.generation(AtlasFormat::R8),
        1,
        "an idle texture stays"
    );
    pool.advance_to(EXPIRY_FRAMES);
    commit(&gpu, &mut textures, &mut pool);
    assert!(textures.scratch_view(AtlasFormat::R8, id.idx).is_none());
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
        pool.allocate(format, Tenant::Glyphs, U16Vec2::splat(40))
            .unwrap();
        pool.allocate(format, Tenant::Glyphs, U16Vec2::splat(64))
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
