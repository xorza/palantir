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
fn commit(gpu: &TestGpu, textures: &mut AtlasTextures, pool: &AtlasPool) {
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
    commit(&gpu, &mut textures, &pool);
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
    commit(&gpu, &mut textures, &pool);
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
    commit(&gpu, &mut textures, &pool);
    assert_eq!(textures.generation(AtlasFormat::R8), 2, "nothing changed");
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
    commit(&gpu, &mut textures, &pool);
    fill(
        &gpu,
        textures.page_texture(AtlasFormat::R8, 1).unwrap(),
        64,
        &[0xaa; 64 * 64],
    );

    pool.release(wide);
    pool.close_page(AtlasFormat::R8, 1);
    let generation = textures.generation(AtlasFormat::R8);
    commit(&gpu, &mut textures, &pool);
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
    commit(&gpu, &mut textures, &pool);
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
    commit(&gpu, &mut textures, &pool);
    assert!(textures.scratch_view(AtlasFormat::R8, id.idx).is_some());
    assert_eq!(textures.bytes(AtlasFormat::R8), 128 * 128);
    assert_eq!(textures.generation(AtlasFormat::R8), 1);
    pool.release_scratch(AtlasFormat::R8, id);
    commit(&gpu, &mut textures, &pool);
    assert_eq!(
        textures.generation(AtlasFormat::R8),
        1,
        "an idle texture stays"
    );
    pool.advance_to(EXPIRY_FRAMES);
    commit(&gpu, &mut textures, &pool);
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
    commit(&gpu, &mut textures, &pool);
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
