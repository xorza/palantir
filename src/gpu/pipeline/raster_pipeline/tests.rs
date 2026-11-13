//! The raster program reads the page a quad names under every set of open slots it is built for.

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::page_list::MAX_PAGES;
use crate::gpu::atlas::page_slots::PageSlots;
use crate::gpu::atlas::row_texture::RowTexture;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::device::gpu::FilledBuffer;
use crate::gpu::pipeline::atlas_binding::{ATLAS_GROUP, AtlasGroup, AtlasSlots};
use crate::gpu::pipeline::clip_variant::Clipping;
use crate::gpu::pipeline::instanced_pipeline::VariantTarget;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::raster_pipeline::raster_quad::RasterQuad;
use crate::gpu::pipeline::raster_pipeline::{RasterDraws, RasterPipeline, RasterVariants};
use crate::gpu::pipeline::viewport_push::ViewportPush;
use crate::gpu::resource::frame_group::FrameGroup;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::gpu::texel;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;
use crate::primitives::wgsl::FRAME_GROUP;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use glam::{IVec2, U16Vec2, UVec2, Vec2};

/// The side of every page and item here: one item fills a page.
const SIDE: u16 = 16;

/// The alpha byte page `page` of each kind holds in every texel.
const fn alpha(page: u8) -> u8 {
    40 * (page + 1)
}

/// Mask and color lists of 16² pages, one 16² item a page, so four items open pages 0 to 3; page
/// `k` of each holds an alpha of `40 (k + 1)` in every texel. For each set of slots `s` the
/// pipeline built for mask and color slots `s` draws a white quad over every page of `s` of each
/// kind, a 16² cell apiece, onto a cleared target: a mask quad's alpha is its coverage, a color
/// quad's its texel's alpha, so each cell's alpha byte reads `40 (k + 1)` back exactly, from 40 to
/// 160, and a quad that read another page would read another multiple. The sets: slot 0, 1 and 3
/// alone, where one page sits past slot 0 as a shrink leaves it, 0 and 2, and all four. A census
/// that turns no raster builds the flat pipeline alone; one that turns one builds the spun twin
/// too, which compiles for every set.
#[test]
fn a_quad_reads_its_page_under_every_set_of_slots() {
    let gpu = headless_test_gpu();
    let mut pool = AtlasPool::uniform(u32::from(SIDE), u32::from(SIDE), 0);
    pool.advance_to(1);
    let mut items = |format| -> Vec<u8> {
        (0..MAX_PAGES)
            .map(|_| {
                let handle = pool
                    .allocate(format, Tenant::Glyphs, UVec2::splat(u32::from(SIDE)))
                    .unwrap()
                    .handle;
                pool.placement(handle).unwrap().page
            })
            .collect()
    };
    let pages = [items(AtlasFormat::R8), items(AtlasFormat::Rgba8)];
    for list in &pages {
        assert_eq!(*list, [0, 1, 2, 3], "one item a page");
    }
    let mut atlas = AtlasTextures::new(&gpu);
    gpu.frame(|ctx| atlas.commit(ctx.gpu, ctx.encoder, &mut pool));
    for (format, texel_bytes) in [(AtlasFormat::R8, 1), (AtlasFormat::Rgba8, 4)] {
        for page in 0..MAX_PAGES as u8 {
            let mut bytes = vec![0; usize::from(SIDE).pow(2) * texel_bytes];
            for texel in bytes.chunks_mut(texel_bytes) {
                *texel.last_mut().unwrap() = alpha(page);
            }
            let texture = atlas.page_texture(format, usize::from(page)).unwrap();
            gpu.queue.write_texture(
                texture.as_image_copy(),
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(u32::from(SIDE) * texel_bytes as u32),
                    rows_per_image: None,
                },
                texel::extent(UVec2::splat(u32::from(SIDE))),
            );
        }
    }

    let binding = TextureBinding::new(&gpu);
    let group = AtlasGroup::new(&gpu, &atlas);
    let program = RasterPipeline::new(&gpu, &group.pipeline_layout(&gpu, &binding));
    let frame = FrameGroup::new(&gpu, binding, RowTexture::new(&gpu, 1).row_view());
    let size = UVec2::new(u32::from(SIDE) * MAX_PAGES as u32, 2 * u32::from(SIDE));
    let target = gpu.target("palantir.test.raster.slots", size);
    let mut variants = RasterVariants::NONE;
    for bits in [0b0001, 0b0010, 0b1000, 0b0101, 0b1111] {
        let open = PageSlots::new(bits);
        let slots = AtlasSlots {
            mask: open,
            color: open,
        };
        let variant = VariantTarget {
            format: target.texture().format(),
            clipping: Clipping::Off,
            slots,
        };
        program.ensure(&mut variants, &gpu, variant, RasterDraws::Flat);
        assert!(
            variants
                .pair(Clipping::Off, slots, group.group())
                .spun
                .is_none(),
            "a frame that turns no raster builds no spun pipeline"
        );
        program.ensure(&mut variants, &gpu, variant, RasterDraws::FlatAndSpun);
        assert!(
            variants
                .pair(Clipping::Off, slots, group.group())
                .spun
                .is_some()
        );
        let quads: Vec<RasterQuad> = [ContentType::Mask, ContentType::Color]
            .into_iter()
            .enumerate()
            .flat_map(|(row, kind)| {
                (0..MAX_PAGES as u8)
                    .filter(move |&page| open.holds(page))
                    .map(move |page| RasterQuad {
                        pos: IVec2::new(
                            i32::from(page) * i32::from(SIDE),
                            row as i32 * i32::from(SIDE),
                        ),
                        dim: U16Vec2::splat(SIDE),
                        size: UVec2::splat(u32::from(SIDE)),
                        uv_and_kind: RasterQuad::pack_uv(0, 0, page, kind),
                        color: RgbaF16::WHITE,
                        clip: ClipChain::NONE,
                    })
            })
            .collect();
        let instances = gpu.create_filled_buffer(
            FilledBuffer {
                label: "palantir.test.raster.quads",
                size: size_of_val(quads.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX,
            },
            &[bytemuck::cast_slice(&quads)],
        );
        gpu.frame(|ctx| {
            let pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.test.raster.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut rec = PassRecorder::new(
                pass,
                ViewportPush {
                    size: size.as_vec2(),
                    origin: Vec2::ZERO,
                },
            );
            rec.group(FRAME_GROUP, frame.group());
            rec.pipeline(variants.pair(Clipping::Off, slots, group.group()).flat);
            rec.group(ATLAS_GROUP, group.group());
            rec.vertex(0, &instances);
            rec.draw(0..4, 0..quads.len() as u32);
        });
        let texels = gpu.read_texture(target.texture());
        for row in 0..2 {
            for page in 0..MAX_PAGES as u8 {
                let at = UVec2::new(
                    u32::from(page) * u32::from(SIDE) + 8,
                    row * u32::from(SIDE) + 8,
                );
                let byte = (at.y * size.x + at.x) as usize * 4 + 3;
                let want = if open.holds(page) { alpha(page) } else { 0 };
                assert_eq!(
                    texels[byte],
                    want,
                    "slots {bits:#06b}, {} page {page}",
                    ["mask", "color"][row as usize]
                );
            }
        }
    }
}

/// A frame with no text or icon draws no raster pipeline, whatever it turns; one with some draws
/// the flat one, and the spun one too where an animation turns one.
#[test]
fn a_frame_draws_the_spun_pipeline_only_where_it_turns_a_raster() {
    assert_eq!(RasterDraws::of(false, false), RasterDraws::None);
    assert_eq!(RasterDraws::of(false, true), RasterDraws::None);
    assert_eq!(RasterDraws::of(true, false), RasterDraws::Flat);
    assert_eq!(RasterDraws::of(true, true), RasterDraws::FlatAndSpun);
}
