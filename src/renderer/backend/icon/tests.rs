//! The unload path end to end: what a dropped [`IconSet`] costs the
//! backend to forget, and when.
//!
//! All of it needs a device, because the backend owns a `RasterAtlas` and
//! that owns textures — so the whole file sits behind `internals`, leaving a
//! default `cargo test` GPU-free. Same arrangement as the raster atlas's own
//! suite.

#[cfg(feature = "internals")]
mod gpu {
    use crate::host::test_gpu::headless_test_gpu;
    use crate::icons::icon_atlas::{IconAtlas, IconId};
    use crate::icons::icon_raster_key::IconRasterKey;
    use crate::icons::icon_registry::{IconRegistry, IconSetId};
    use crate::icons::icon_set::IconSet;
    use crate::renderer::backend::icon::IconBackend;
    use glam::Vec2;
    use std::rc::Rc;

    /// One tintable icon, so the raster lands on the mask side and the whole
    /// path — parse, rasterize, pack — runs.
    const SOLID: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#000"/></svg>"##;

    /// One frame boundary the way `WgpuBackend::submit` drives it, on a
    /// clock this test owns in place of the shaper's.
    fn tick(backend: &mut IconBackend, frame: &mut u64) {
        *frame += 1;
        backend.end_frame(*frame);
    }

    fn load(icons: &IconRegistry) -> IconSet {
        icons.register(Rc::new(IconAtlas::from_svgs([("solid", SOLID)])))
    }

    /// Draw `set`'s only icon at 16², which is what puts a parse in the
    /// rasterizer and a raster in the atlas.
    fn draw(backend: &mut IconBackend, device: &wgpu::Device, set: &IconSet) {
        let icon = set.handle(IconId(0)).icon;
        let key = IconRasterKey::for_box(icon, Vec2::splat(16.0));
        assert!(
            backend.slot(device, key).is_some(),
            "the fixture icon must rasterize",
        );
    }

    /// The whole reason [`IconSet`] became an owner: everything the backend
    /// caches for a set is keyed on its [`IconSetId`], and nothing else can
    /// tell that those keys are dead rather than merely cold.
    #[test]
    fn dropping_a_set_unloads_its_parses_and_its_rasters_at_the_next_submit() {
        let gpu = headless_test_gpu();
        let icons = IconRegistry::default();
        let mut backend = IconBackend::new(&gpu.device, icons.clone());
        let mut frame = 0u64;

        let set = load(&icons);
        draw(&mut backend, &gpu.device, &set);
        assert_eq!(backend.rasterizer.parsed_count(), 1);
        assert_eq!(backend.atlas.cache.len(), 1);

        // Held: submits come and go and the set stays loaded.
        tick(&mut backend, &mut frame);
        tick(&mut backend, &mut frame);
        assert_eq!(backend.rasterizer.parsed_count(), 1);
        assert_eq!(backend.atlas.cache.len(), 1);

        // Dropped: queued now, forgotten at the submit — the backend has no
        // other point at which it is allowed to touch the caches.
        drop(set);
        assert_eq!(
            backend.rasterizer.parsed_count(),
            1,
            "the drop itself must not reach into the backend",
        );
        tick(&mut backend, &mut frame);
        assert_eq!(
            backend.rasterizer.parsed_count(),
            0,
            "the parsed SVG went with the set",
        );
        assert_eq!(backend.atlas.cache.len(), 0, "and so did its raster");
        assert!(
            (0..icons.slot_count()).all(|slot| icons.resident(slot).is_none()),
            "and the registry freed the slot",
        );
    }

    /// A set loaded into the slot a released one held must not inherit its
    /// rasters. The generation is what keeps the two apart, and the unload has
    /// to happen before the slot is handed on — which is why the drain frees
    /// the slot and tells the backend in the same call.
    #[test]
    fn a_set_reusing_a_freed_slot_rasterizes_from_scratch() {
        let gpu = headless_test_gpu();
        let icons = IconRegistry::default();
        let mut backend = IconBackend::new(&gpu.device, icons.clone());
        let mut frame = 0u64;

        let first = load(&icons);
        assert_eq!(first.handle(IconId(0)).icon.set, IconSetId::new(0, 0));
        draw(&mut backend, &gpu.device, &first);
        drop(first);
        tick(&mut backend, &mut frame);

        let second = load(&icons);
        assert_eq!(
            second.handle(IconId(0)).icon.set,
            IconSetId::new(0, 1),
            "the fixture only proves anything if the slot was reused",
        );
        assert_eq!(backend.atlas.cache.len(), 0, "nothing carried over");

        draw(&mut backend, &gpu.device, &second);
        assert_eq!(backend.rasterizer.parsed_count(), 1);
        assert_eq!(backend.atlas.cache.len(), 1, "one raster, freshly made");
    }

    /// A caller that builds a fresh atlas inside its frame closure loads a set
    /// per frame. Every one is released as the previous `IconSet` drops, so the
    /// backend's two per-set caches stay at one set's worth however long it
    /// runs — the leak this whole mechanism exists to close.
    #[test]
    fn loading_a_fresh_set_every_frame_holds_the_backend_caches_flat() {
        let gpu = headless_test_gpu();
        let icons = IconRegistry::default();
        let mut backend = IconBackend::new(&gpu.device, icons.clone());
        let mut frame = 0u64;

        let mut held: Option<IconSet> = None;
        for _ in 0..32 {
            let next = load(&icons);
            draw(&mut backend, &gpu.device, &next);
            // Assigning is what drops the previous frame's set.
            held = Some(next);
            tick(&mut backend, &mut frame);
            // One live set's worth, whatever the frame number: the previous
            // frame's parse and raster were reclaimed by this submit.
            assert_eq!(
                (backend.rasterizer.parsed_count(), backend.atlas.cache.len()),
                (1, 1),
                "frame {frame} retained a dead set's caches",
            );
            let live = (0..icons.slot_count())
                .filter(|&slot| icons.resident(slot).is_some())
                .count();
            assert_eq!(live, 1, "frame {frame} left a set resident");
        }
        drop(held);
        tick(&mut backend, &mut frame);
        assert_eq!(
            (backend.rasterizer.parsed_count(), backend.atlas.cache.len()),
            (0, 0),
            "and the last one goes when its set does",
        );
    }
}
