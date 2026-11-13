//! The unload path: what dropping an [`IconSet`] costs the backend to forget, and when. Needs a device.

mod gpu {
    use crate::common::counters::CounterSet;
    use crate::gpu::atlas::AtlasPool;
    use crate::gpu::atlas::atlas_format::AtlasFormat;
    use crate::gpu::atlas::page_list::PageListConfig;
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::raster::icon_backend::IconBackend;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::icons::icon_raster_key::IconRasterKey;
    use crate::icons::icon_registry::IconRegistry;
    use crate::icons::icon_set::IconSet;
    use crate::icons::icon_table::IconTable;
    use crate::primitives::identity::icon_id::IconId;
    use crate::primitives::identity::icon_set_id::IconSetId;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::render_buffer::clip_chain::ClipChain;
    use crate::renderer::render_buffer::icon_draw_row::IconDrawRow;
    use glam::{IVec2, UVec2, Vec2};
    use std::rc::Rc;

    /// A backend over `icons`; see the text suite's peer.
    fn icon_backend(gpu: &Gpu, icons: &IconRegistry) -> IconBackend {
        IconBackend::new(gpu, icons.clone())
    }

    /// One tintable icon, so the raster lands on the mask side.
    const SOLID: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#000"/></svg>"##;

    /// One frame boundary as `WgpuBackend::submit` drives it, on a clock this test owns.
    fn tick(backend: &mut IconBackend, pool: &mut AtlasPool, frame: &mut u64) {
        *frame += 1;
        backend.end_frame(pool, *frame);
    }

    fn load(icons: &IconRegistry) -> IconSet {
        icons.register(Rc::new(IconTable::from_svgs([("solid", SOLID)]).unwrap()))
    }

    /// Draw `set`'s only icon at 16², putting a parse in the rasterizer and a raster in the atlas.
    fn draw(backend: &mut IconBackend, pool: &mut AtlasPool, set: &IconSet) {
        let icon = set.handle(IconId::new(0)).icon;
        let key = IconRasterKey::for_box(icon, Vec2::splat(16.0));
        assert!(
            backend.slot(pool, key).is_some(),
            "the fixture icon must rasterize",
        );
    }

    /// Backend caches are keyed on [`IconSetId`]; only the owner's drop can tell they are dead.
    #[test]
    fn dropping_a_set_unloads_its_parses_and_its_rasters_at_the_next_submit() {
        let gpu = headless_test_gpu();
        let icons = IconRegistry::default();
        let mut backend = icon_backend(&gpu, &icons);
        let mut pool = AtlasPool::new(16384);
        let mut frame = 0u64;

        let set = load(&icons);
        draw(&mut backend, &mut pool, &set);
        assert_eq!(backend.rasterizer.parsed_count(), 1);
        assert_eq!(backend.pass.atlas.cached_len(), 1);

        // Held: the set stays loaded across submits.
        tick(&mut backend, &mut pool, &mut frame);
        tick(&mut backend, &mut pool, &mut frame);
        assert_eq!(backend.rasterizer.parsed_count(), 1);
        assert_eq!(backend.pass.atlas.cached_len(), 1);

        // Dropped: queued now, forgotten at the next submit, the backend's only point to touch caches.
        drop(set);
        assert_eq!(
            backend.rasterizer.parsed_count(),
            1,
            "the drop itself must not reach into the backend",
        );
        tick(&mut backend, &mut pool, &mut frame);
        assert_eq!(
            backend.rasterizer.parsed_count(),
            0,
            "the parsed SVG went with the set",
        );
        assert_eq!(backend.pass.atlas.cached_len(), 0, "and so did its raster");
        assert_eq!(
            pool.list(AtlasFormat::R8).page(0).unwrap().live_texels(),
            0,
            "given back to the pool"
        );
        assert!(
            icons.resident_ids().is_empty(),
            "and the registry freed the slot",
        );
    }

    /// An icon whose raster passes a page takes a slot that draws nothing: on pages of 8², a 16²
    /// raster is cached and counted once, and its batch draws no quad, this frame or the next,
    /// which rasterizes nothing again.
    #[test]
    fn an_icon_past_a_page_draws_nothing_and_rasterizes_once() {
        let gpu = headless_test_gpu();
        let icons = IconRegistry::default();
        let mut backend = icon_backend(&gpu, &icons);
        let config = PageListConfig {
            first_edge: 8,
            page_edge: 8,
            soft_bytes: 1 << 20,
        };
        let mut pool = AtlasPool::with_configs([config; 3], 16384);
        let mut frame = 0u64;
        let set = load(&icons);
        let row = IconDrawRow {
            key: IconRasterKey::for_box(set.handle(IconId::new(0)).icon, Vec2::splat(16.0)),
            origin: IVec2::ZERO,
            size: UVec2::splat(16),
            color: RgbaF16::WHITE,
            desaturate: false,
            spin: None,
            clip: ClipChain::NONE,
        };
        for _ in 0..2 {
            backend.prepare_batch(&mut pool, 0, &[row]);
            assert_eq!(backend.pass.instance_count(), 0, "no quad");
            assert_eq!(backend.pass.atlas.cached_len(), 1, "the key is cached");
            assert_eq!(
                backend.pass.atlas.counters.counts().oversized,
                1,
                "rasterized once"
            );
            tick(&mut backend, &mut pool, &mut frame);
        }
        assert!(
            pool.list(AtlasFormat::R8).page(0).is_none(),
            "no page opened"
        );
    }

    /// A set loaded into a released slot must not inherit its rasters: the generation separates them, and the drain unloads before freeing the slot.
    #[test]
    fn a_set_reusing_a_freed_slot_rasterizes_from_scratch() {
        let gpu = headless_test_gpu();
        let icons = IconRegistry::default();
        let mut backend = icon_backend(&gpu, &icons);
        let mut pool = AtlasPool::new(16384);
        let mut frame = 0u64;

        let first = load(&icons);
        assert_eq!(first.handle(IconId::new(0)).icon.set, IconSetId::new(0, 0));
        draw(&mut backend, &mut pool, &first);
        drop(first);
        tick(&mut backend, &mut pool, &mut frame);

        let second = load(&icons);
        assert_eq!(
            second.handle(IconId::new(0)).icon.set,
            IconSetId::new(0, 1),
            "the fixture only proves anything if the slot was reused",
        );
        assert_eq!(backend.pass.atlas.cached_len(), 0, "nothing carried over");

        draw(&mut backend, &mut pool, &second);
        assert_eq!(backend.rasterizer.parsed_count(), 1);
        assert_eq!(
            backend.pass.atlas.cached_len(),
            1,
            "one raster, freshly made"
        );
    }

    /// A fresh atlas per frame loads a set per frame; each is released as the previous drops, so the per-set caches stay at one set.
    #[test]
    fn loading_a_fresh_set_every_frame_holds_the_backend_caches_flat() {
        let gpu = headless_test_gpu();
        let icons = IconRegistry::default();
        let mut backend = icon_backend(&gpu, &icons);
        let mut pool = AtlasPool::new(16384);
        let mut frame = 0u64;

        let mut held: Option<IconSet> = None;
        for _ in 0..32 {
            let next = load(&icons);
            draw(&mut backend, &mut pool, &next);
            // Assigning drops the previous frame's set.
            held = Some(next);
            tick(&mut backend, &mut pool, &mut frame);
            // One live set's worth: the previous frame's parse and raster were reclaimed by this submit.
            assert_eq!(
                (
                    backend.rasterizer.parsed_count(),
                    backend.pass.atlas.cached_len()
                ),
                (1, 1),
                "frame {frame} retained a dead set's caches",
            );
            assert_eq!(
                icons.resident_ids().len(),
                1,
                "frame {frame} left a set resident"
            );
        }
        drop(held);
        tick(&mut backend, &mut pool, &mut frame);
        assert_eq!(
            (
                backend.rasterizer.parsed_count(),
                backend.pass.atlas.cached_len()
            ),
            (0, 0),
            "and the last one goes when its set does",
        );
    }
}
