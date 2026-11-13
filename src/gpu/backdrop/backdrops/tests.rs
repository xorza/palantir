use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::display::Display;
use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::page_list::PageListConfig;
use crate::gpu::backdrop::backdrops::{
    BackdropFrame, BackdropSlot, Backdrops, Held, Kept, RESULTS, SpectrumCell, held_clear, place,
};
use crate::gpu::backdrop::tests::CLEAR;
use crate::gpu::pipeline::effect_pipeline::backdrop_groups::ResultTexture;
use crate::gpu::render_owner_id::{RenderOwnerId, RenderOwners};
use crate::gpu::resource::shader_body::ShaderBody;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::test_gpu::{TestGpu, headless_test_gpu};
use crate::gpu::texel::TargetFormat;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::{BackdropRow, BackdropSource};
use crate::shape::paint::blur_plan::BlurPlan;
use glam::UVec2;
use std::mem::offset_of;

const SURFACE: UVec2 = UVec2::new(200, 100);

fn key(i: u32) -> BackdropKey {
    BackdropKey {
        owner: WidgetId::from_hash("backdrop"),
        shape: i,
    }
}

/// Backdrop `i` over x `10 + 40 i ..`, at σ 2, taking its result from `source`: a 30² result whose
/// slot rounds up to 32².
fn row(i: u32, source: BackdropSource) -> BackdropRow {
    BackdropRow::fixture(i, URect::new(10 + 40 * i, 10, 30, 30), BlurPlan::new(2.0))
        .with_source(source)
}

/// [`row`] capturing after `i + 1` groups.
fn captured(i: u32) -> BackdropRow {
    row(i, BackdropSource::Capture { barrier: i + 1 })
}

/// A half-float list of pages that open at `first_edge` and double to `page_edge`, past a soft
/// budget of `soft_bytes`.
fn pool(first_edge: u32, page_edge: u32, soft_bytes: u64) -> AtlasPool {
    let config = PageListConfig {
        first_edge,
        page_edge,
        soft_bytes,
    };
    AtlasPool::with_configs([config; 3], 16384)
}

/// Backdrops over a pool and its textures, one display frame a prepare, as the backend runs them:
/// the prepare, the pool's commit, the follow, and the drain of the pool's evictions that ends a
/// submit.
#[derive(Debug)]
struct Rig {
    gpu: TestGpu,
    backdrops: Backdrops,
    pool: AtlasPool,
    textures: AtlasTextures,
    frame: u64,
}

impl Rig {
    fn new(pool: AtlasPool) -> Self {
        let gpu = headless_test_gpu();
        Self {
            backdrops: Backdrops::new(&TextureBinding::new(&gpu)),
            textures: AtlasTextures::new(&gpu),
            gpu,
            pool,
            frame: 0,
        }
    }

    /// Prepare the next display frame of `owner` holding `rows`, then commit the pool and follow
    /// it.
    fn prepare(&mut self, owner: RenderOwnerId, rows: &[BackdropRow]) {
        self.frame += 1;
        self.pool.advance_to(self.frame);
        let mut buffer = RenderBuffer::new();
        buffer.display = Display::from_physical(SURFACE, 1.0);
        buffer.backdrops.extend_from_slice(rows);
        buffer
            .live_backdrops
            .refill(rows.iter().map(|row| row.capture.key));
        let Self {
            gpu,
            backdrops,
            pool,
            textures,
            ..
        } = self;
        gpu.frame(|ctx| {
            backdrops.prepare(
                ctx,
                BackdropFrame {
                    buffer: &buffer,
                    owner,
                    format: TargetFormat::RGBA8_SRGB,
                    clear: CLEAR.to_f16(),
                    backbuffer: None,
                },
                pool,
            );
            textures.commit(ctx.gpu, ctx.encoder, pool);
            backdrops.follow(ctx.gpu, textures);
        });
        backdrops.end_frame(pool);
    }

    /// Skip `frames` display frames no frame of these backdrops prepares in.
    const fn skip(&mut self, frames: u64) {
        self.frame += frames;
    }

    /// Where `owner`'s backdrop `i`'s result lies, if it holds one: its texture and its texels.
    fn slot(&self, owner: RenderOwnerId, i: u32) -> Option<(ResultTexture, URect)> {
        match *self.backdrops.cache.get(&(owner, key(i)))? {
            Kept::Slot(slot) => {
                let placed = place(slot, &self.pool)?;
                Some((
                    placed.texture,
                    URect {
                        min: placed.origin,
                        size: slot.size,
                    },
                ))
            }
            Kept::Clear(_) => None,
        }
    }

    /// The edge of the half-float list's page `idx`.
    fn page_edge(&self, idx: usize) -> u32 {
        self.pool.list(RESULTS).page(idx).unwrap().edge()
    }
}

/// Two windows' backdrops under one key keep a result each, in slots apart: a frame of one leaves
/// the other's, its backdrops leaving drops only its own, and retiring a window drops the rest of
/// its, its texels back in the page. A frame with no backdrop makes no resources; the first that
/// captures makes the composite's groups and the blur, and both stay.
#[test]
fn each_window_keeps_its_own_results() {
    let mut owners = RenderOwners::default();
    let (a, b) = (owners.reserve(), owners.reserve());
    let mut rig = Rig::new(AtlasPool::new(16384));
    rig.prepare(a, &[]);
    assert!(
        rig.backdrops.resources.is_none(),
        "no backdrop, no resources"
    );
    rig.prepare(a, &[captured(0)]);
    let resources = rig.backdrops.resources.as_ref().unwrap();
    assert!(resources.blur.is_some());
    assert!(resources.groups.is_some());
    rig.prepare(b, &[captured(0)]);
    let (at_a, at_b) = (rig.slot(a, 0).unwrap(), rig.slot(b, 0).unwrap());
    assert_eq!(at_a.0, ResultTexture::Page(0));
    assert_eq!(at_b.0, ResultTexture::Page(0));
    assert!(!at_a.1.intersects(at_b.1), "{at_a:?} and {at_b:?} apart");
    assert_eq!(rig.backdrops.blurs(), 1);

    rig.prepare(a, &[row(0, BackdropSource::Held)]);
    assert_eq!(rig.backdrops.blurs(), 0);
    assert_eq!(rig.slot(a, 0), Some(at_a), "held");
    rig.prepare(a, &[]);
    assert!(!rig.backdrops.cache.contains_key(&(a, key(0))), "left");
    assert_eq!(rig.slot(b, 0), Some(at_b), "the other window's");
    rig.backdrops.retire(b, &mut rig.pool);
    assert!(rig.backdrops.cache.is_empty(), "retired");
    assert_eq!(rig.pool.list(RESULTS).page(0).unwrap().live_texels(), 0);
    assert!(
        rig.backdrops
            .resources
            .as_ref()
            .is_some_and(|r| r.blur.is_some()),
        "kept for the next backdrop"
    );
}

/// A backdrop that shows the clear holds the clear as the target holds it and frees its slot, which
/// the next capture of the same size takes; a held backdrop of the clear keeps it without a blur.
#[test]
fn a_backdrop_of_the_clear_frees_its_slot() {
    let owner = RenderOwners::default().reserve();
    let mut rig = Rig::new(AtlasPool::new(16384));
    rig.prepare(owner, &[captured(0)]);
    let first = rig.slot(owner, 0).unwrap();
    rig.prepare(owner, &[row(0, BackdropSource::Clear)]);
    assert_eq!(rig.backdrops.blurs(), 0);
    let held = held_clear(CLEAR.to_f16(), TargetFormat::RGBA8_SRGB);
    assert!(matches!(
        rig.backdrops.cache[&(owner, key(0))],
        Kept::Clear(clear) if clear == held
    ));
    rig.prepare(owner, &[row(0, BackdropSource::Held), captured(1)]);
    assert_eq!(rig.backdrops.blurs(), 1);
    assert!(matches!(
        rig.backdrops.cache[&(owner, key(0))],
        Kept::Clear(_)
    ));
    assert_eq!(rig.slot(owner, 1), Some(first));
}

/// An 8-bit sRGB target holds the clear as its byte decoded, and a float one as the `f16` clear:
/// each rounded to `f16`, as the blur stores its levels.
#[test]
fn the_clear_a_backdrop_shows_is_the_one_the_target_holds() {
    let byte = RgbaF32::from(RgbaF32::from(SrgbaU8::rgb(51, 115, 178)).to_f16());
    assert_eq!(
        held_clear(CLEAR.to_f16(), TargetFormat::RGBA8_SRGB),
        [byte.r, byte.g, byte.b]
    );
    let float = RgbaF32::from(CLEAR.to_f16());
    assert_ne!(float.r, byte.r, "the byte rounds the clear");
    assert_eq!(
        held_clear(CLEAR.to_f16(), TargetFormat::RGBA16_FLOAT),
        [float.r, float.g, float.b]
    );
}

/// `BackdropSlot` lies as the backdrop effect contract declares `PalantirBackdrop`, and
/// `SpectrumCell` as it declares `PalantirSpectrumCell`, which storage bindings read unchecked: the
/// slot's clear, a `vec3` the shader aligns to 16 bytes, starts at 16 after the lattice and the
/// ceiling, and `reads` at 32 after the floor; each of a cell's three `vec3`s takes 16 bytes, 48 in
/// all.
#[test]
fn the_backdrop_buffers_lie_as_the_shader_reads_them() {
    let contract = ShaderBody {
        label: "palantir.effect.backdrop",
        wgsl: include_str!("../../../effect/backdrop.wgsl"),
        includes: &[include_str!("../../../effect/contract.wgsl")],
        constants: &[],
    };
    contract.assert_struct_layout(
        "PalantirSpectrumCell",
        size_of::<SpectrumCell>(),
        &[
            ("weight", offset_of!(SpectrumCell, weight)),
            ("cumulative", offset_of!(SpectrumCell, cumulative)),
            ("moment", offset_of!(SpectrumCell, moment)),
        ],
    );
    let members = [
        ("offset", offset_of!(BackdropSlot, offset)),
        ("lattice", offset_of!(BackdropSlot, lattice)),
        ("max_luminance", offset_of!(BackdropSlot, max_luminance)),
        ("clear", offset_of!(BackdropSlot, clear)),
        ("min_luminance", offset_of!(BackdropSlot, min_luminance)),
        ("reads", offset_of!(BackdropSlot, reads)),
        ("fill", offset_of!(BackdropSlot, fill)),
        ("fill_axis", offset_of!(BackdropSlot, fill_axis)),
        ("border_color", offset_of!(BackdropSlot, border_color)),
        ("border_width", offset_of!(BackdropSlot, border_width)),
        ("fill_kind", offset_of!(BackdropSlot, fill_kind)),
        ("fill_lut_row", offset_of!(BackdropSlot, fill_lut_row)),
    ];
    assert_eq!(offset_of!(BackdropSlot, clear), 16);
    assert_eq!(offset_of!(BackdropSlot, reads), 32);
    assert_eq!(offset_of!(BackdropSlot, fill_kind), 100);
    assert_eq!(size_of::<BackdropSlot>(), 112);
    contract.assert_struct_layout("PalantirBackdrop", size_of::<BackdropSlot>(), &members);
}

/// Past its soft budget the pool evicts a result no frame read this display frame or the last,
/// from any window, and lists it for that window's ledger; with none such it grows. Pages of 32²
/// that double to 64, under a budget of one 32² page of `Rgba16Float`: window a's result takes the
/// first page whole. Two frames on, window b's evicts it and takes its texels, the page still 32
/// wide. b's second backdrop the frame after finds only b's own result, read this frame, so the
/// page doubles to 64 and both stay.
#[test]
fn past_its_budget_the_pool_evicts_before_it_grows() {
    let mut owners = RenderOwners::default();
    let (a, b) = (owners.reserve(), owners.reserve());
    let mut rig = Rig::new(pool(32, 64, 32 * 32 * 8));
    rig.prepare(a, &[captured(0)]);
    assert_eq!(rig.page_edge(0), 32);
    let at_a = rig.slot(a, 0).unwrap();

    rig.skip(1);
    rig.prepare(b, &[captured(0)]);
    assert_eq!(rig.backdrops.evicted(a).collect::<Vec<_>>(), [key(0)]);
    assert!(rig.slot(a, 0).is_none(), "a's result went");
    assert_eq!(rig.slot(b, 0), Some(at_a), "a's texels, freed");
    assert_eq!(rig.page_edge(0), 32);

    rig.prepare(b, &[captured(0), captured(1)]);
    assert_eq!(rig.page_edge(0), 64, "grown for the frame's own");
    assert_eq!(rig.backdrops.evicted(b).count(), 0);
    assert!(rig.slot(b, 1).is_some());

    rig.backdrops.forget_evicted(a);
    assert_eq!(rig.backdrops.evicted(a).count(), 0);
}

/// A held result the pool evicted before its frame shows the clear and is listed for its ledger,
/// which captures it again: window a's result fills a 32² page; two frames on, window b's first
/// result evicts it, past a soft budget of nothing, and its second grows the page; a's next frame
/// reads it as held.
#[test]
fn a_held_result_the_pool_evicted_shows_the_clear() {
    let mut owners = RenderOwners::default();
    let (a, b) = (owners.reserve(), owners.reserve());
    let mut rig = Rig::new(pool(32, 64, 0));
    rig.prepare(a, &[captured(0)]);
    rig.skip(1);
    rig.prepare(b, &[captured(0), captured(1)]);
    rig.backdrops.forget_evicted(a);
    rig.prepare(a, &[row(0, BackdropSource::Held)]);
    assert!(rig.slot(a, 0).is_none());
    assert_eq!(rig.backdrops.slots[0].lattice, 0.0, "it shows the clear");
    assert_eq!(rig.backdrops.evicted(a).collect::<Vec<_>>(), [key(0)]);
}

/// A result within half a page lands on a page, and one past it takes a scratch texture of its
/// own on the ladder. Under the starting constants, pages of 1024: a 400 × 300 result takes page 0;
/// a 1936 × 1088 one a scratch texture of `2048 × 1536`, as 1936 rounds to 2048 and 1088 to the
/// midpoint 1536; once its backdrop goes, a 1900 × 1000 result, `2048 × 1024` on the ladder, reuses
/// the idle texture within twice its area; and once that one goes, the texture frees an expiry
/// window later.
#[test]
fn a_large_result_takes_a_scratch_texture_of_its_own() {
    let owner = RenderOwners::default().reserve();
    let mut rig = Rig::new(AtlasPool::new(16384));
    let pool = &mut rig.pool;
    let backdrops = &mut rig.backdrops;
    let (small, _) = backdrops
        .slot_for((owner, key(0)), UVec2::new(400, 300), pool)
        .unwrap();
    assert!(matches!(small.held, Held::Item(_)));
    let (large, placed) = backdrops
        .slot_for((owner, key(1)), UVec2::new(1936, 1088), pool)
        .unwrap();
    let Held::Scratch(id) = large.held else {
        panic!("a large result is scratch: {large:?}");
    };
    assert_eq!(placed.texture, ResultTexture::Scratch(id));
    assert_eq!(placed.origin, UVec2::ZERO);
    assert_eq!(pool.scratch_size(RESULTS, id), Some(UVec2::new(2048, 1536)));
    backdrops.cache.insert((owner, key(1)), Kept::Slot(large));
    backdrops.drop_result((owner, key(1)), pool);
    let (again, _) = backdrops
        .slot_for((owner, key(2)), UVec2::new(1900, 1000), pool)
        .unwrap();
    let Held::Scratch(reused) = again.held else {
        panic!("a large result is scratch: {again:?}");
    };
    assert_eq!(reused.idx, id.idx, "the idle texture reused");
    assert_eq!(
        pool.scratch_size(RESULTS, reused),
        Some(UVec2::new(2048, 1536))
    );
    backdrops.cache.insert((owner, key(2)), Kept::Slot(again));
    backdrops.drop_result((owner, key(2)), pool);
    pool.advance_to(EXPIRY_FRAMES - 1);
    assert_eq!(pool.list(RESULTS).scratch().texels(), 2048 * 1536);
    pool.advance_to(EXPIRY_FRAMES);
    assert_eq!(pool.list(RESULTS).scratch().texels(), 0);
}

/// A frame with a result on a page and one in a scratch texture reads each through a group over
/// its own texture: page 0's, and the scratch slot's, two groups apart. A 40² surface region at
/// σ 2 on pages of 64 takes a slot of 48², past half a page. A backdrop that shows its clear reads
/// none, through the stand-in's group. The large backdrop showing the clear and capturing again,
/// three times over, takes its idle texture back each time under a new id, and its composite
/// keeps the one group of that slot.
#[test]
fn a_scratch_result_reads_through_a_group_of_its_own() {
    let owner = RenderOwners::default().reserve();
    let mut rig = Rig::new(pool(64, 64, 1 << 20));
    let large = BackdropRow::fixture(5, URect::new(100, 20, 40, 40), BlurPlan::new(2.0))
        .with_source(BackdropSource::Capture { barrier: 2 });
    rig.prepare(owner, &[captured(0), large]);
    let Some((ResultTexture::Scratch(id), _)) = rig.slot(owner, 5) else {
        panic!("the large result is scratch");
    };
    let groups = rig.backdrops.groups().unwrap();
    let page = Some(ResultTexture::Page(0));
    let scratch = Some(ResultTexture::Scratch(id));
    assert_eq!(groups.read_of(0), page);
    assert_eq!(groups.read_of(1), scratch);
    let [absent, on_page, in_scratch] = [None, page, scratch].map(|read| groups.group(read));
    assert!(!std::ptr::eq(absent, on_page));
    assert!(!std::ptr::eq(on_page, in_scratch));
    assert!(!std::ptr::eq(absent, in_scratch));
    rig.prepare(owner, &[captured(0).with_source(BackdropSource::Clear)]);
    assert_eq!(rig.backdrops.groups().unwrap().read_of(0), None);
    rig.prepare(owner, &[captured(0), large]);
    assert_eq!(
        rig.pool.list(AtlasFormat::Rgba16F).scratch().texels(),
        64 * 64
    );

    for _ in 0..3 {
        let clear = large.with_source(BackdropSource::Clear);
        rig.prepare(owner, &[captured(0), clear]);
        rig.prepare(owner, &[captured(0), large]);
    }
    let Some((ResultTexture::Scratch(again), _)) = rig.slot(owner, 5) else {
        panic!("the large result is scratch");
    };
    assert_ne!(again, id, "taken again under a new id");
    assert_eq!(again.idx, id.idx, "of the same slot");
    let resources = rig.backdrops.resources.as_ref().unwrap();
    let groups = resources.groups.as_ref().unwrap();
    assert_eq!(groups.scratch.iter().flatten().count(), 1);
    assert_eq!(
        rig.pool.list(AtlasFormat::Rgba16F).scratch().texels(),
        64 * 64
    );
}
