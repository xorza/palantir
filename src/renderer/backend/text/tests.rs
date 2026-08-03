//! Text-backend tests: GPU-wire layout pins for `GlyphInstance` and the
//! shared immediate region, plus the GPU regression suite covering the
//! encoded-glyph cache (liveness, clipping) and the atlas empty-entry
//! sweep.
//!
//! The GPU suite and its `make_inner_run` fixture stay gated on
//! `internals` rather than bare `test`, so the default headless
//! `cargo test` remains GPU-free — matching the visual suite and the
//! atlas bench.

#[cfg(feature = "internals")]
mod internals {
    use crate::primitives::color::ColorU8;
    use crate::primitives::urect::URect;
    use crate::renderer::render_buffer::text::TextDrawRow;
    use crate::scene::record_store::RecordStore;
    use crate::text::request::TextShapeRequest;
    use crate::text::shaped_ref::ShapedTextRef;
    use crate::text::shaper::TextShaper;
    use crate::text::{FontFamily, FontWeight};
    use glam::{UVec2, Vec2};

    #[allow(clippy::too_many_arguments)]
    pub(super) fn make_inner_run(
        store: &RecordStore,
        shaper: &TextShaper,
        text: &str,
        font_size_px: f32,
        line_height_px: f32,
        origin: Vec2,
        viewport: UVec2,
        scale: f32,
        color: ColorU8,
    ) -> TextDrawRow {
        let recorded = store.record_text(store.intern_str(text));
        let request = TextShapeRequest::unbounded(
            text,
            font_size_px,
            line_height_px,
            FontFamily::Sans,
            FontWeight::Regular,
        );
        shaper.layout(request);
        TextDrawRow {
            text: ShapedTextRef::new(request.key, &recorded),
            origin,
            bounds: URect::new(0, 0, viewport.x, viewport.y),
            color,
            scale,
        }
    }
}

mod wire {
    use crate::renderer::backend::text::{GlyphInstance, PARAMS_OFFSET};
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn glyph_instance_is_20_bytes() {
        assert_eq!(size_of::<GlyphInstance>(), 20);
        assert_eq!(align_of::<GlyphInstance>(), 4);
        assert_eq!(offset_of!(GlyphInstance, pos), 0);
        assert_eq!(offset_of!(GlyphInstance, dim), 8);
        assert_eq!(offset_of!(GlyphInstance, uv_and_kind), 12);
        assert_eq!(offset_of!(GlyphInstance, color), 16);
    }

    /// The viewport and the atlas sizes share one immediate region, and
    /// the shader reads them as `Immediates { viewport, params }`.
    ///
    /// Their *adjacency* is no longer worth asserting — `PARAMS_OFFSET`
    /// is defined as `ViewportPush::BYTES`, so a test comparing the two
    /// would restate the definition. (It used to be the literal `8`,
    /// which is why that assertion, and a second one pinning
    /// `PARAMS_BYTES == 8`, existed at all.) What a wider viewport or a
    /// third params field can still break is the pair fitting inside the
    /// region at all, which nothing else checks.
    #[test]
    fn viewport_and_params_fit_the_immediate_region() {
        use crate::renderer::backend::IMMEDIATES_BYTES;
        // The type of `TextBackend::atlas_px`, which `render_batch`
        // writes with `bytemuck::bytes_of` — so the width is the field's
        // and needs no constant of its own beside it.
        let params = size_of::<[u32; 2]>();
        assert!(PARAMS_OFFSET as usize + params <= IMMEDIATES_BYTES as usize);
    }
}

/// GPU regression coverage for the text backend caches (encoded-cache
/// liveness + clipping, atlas empty-entry sweep). Gated on `internals`
/// (not bare `test`) so the default headless `cargo test` stays
/// GPU-free, matching the visual / atlas-bench gating.
#[cfg(feature = "internals")]
mod gpu_regression {
    use wgpu::util::StagingBelt;

    use crate::host::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
    use crate::primitives::color::ColorU8;
    use crate::primitives::span::Span;
    use crate::primitives::urect::URect;
    use crate::renderer::backend::gpu_ctx::GpuCtx;
    use crate::renderer::backend::queue::Queue;
    use crate::renderer::backend::text::TextBackend;
    use crate::renderer::backend::text::tests::internals::make_inner_run;
    use crate::renderer::render_buffer::text::TextDrawRow;
    use crate::scene::record_store::RecordStore;
    use crate::text::shaper::TextShaper;
    use glam::{UVec2, Vec2};

    const PHYSICAL: UVec2 = UVec2::new(640, 480);

    #[derive(Debug)]
    struct TestGpu {
        queue: Queue,
        lease: HeadlessTestGpuLease,
    }

    fn test_gpu() -> TestGpu {
        let lease = headless_test_gpu();
        let queue = Queue::new(lease.queue.clone());
        TestGpu { queue, lease }
    }

    fn run_one_frame(
        device: &wgpu::Device,
        queue: &Queue,
        backend: &mut TextBackend,
        store: &RecordStore,
        scale: f32,
        runs: &[TextDrawRow],
    ) {
        let mut belt = StagingBelt::new(device.clone(), 1 << 16);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut ctx = GpuCtx::new(device, queue, &mut belt, &mut encoder);
            let payloads = store.payloads.borrow();
            let interned_text = payloads.interned_text();
            backend.prepare_batch(&mut ctx, scale, 0, runs, &interned_text);
            backend.flush(&mut ctx);
        }
        belt.finish_and_recall_on_submit(&encoder);
        queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("poll");
    }

    /// A run that hits the encoded cache must still refresh the LRU
    /// `last_use` of every atlas slot it rides. Before the fix the
    /// fast path emitted cached uv coords without touching the slots,
    /// so a steadily-cached run's slots froze at their rasterization
    /// frame and `evict_one` (which fires under zoom's many-sizes
    /// atlas pressure) would reclaim a still-live slot and overwrite it
    /// with a different glyph — garbled text.
    #[test]
    fn cached_run_keeps_its_atlas_slots_live() {
        let gpu = test_gpu();
        let shaper = TextShaper::new();
        let store = RecordStore::default();
        let mut backend = TextBackend::new(&gpu.lease.device, shaper.clone());

        let runs = [make_inner_run(
            &store,
            &shaper,
            "File",
            14.0,
            14.0 * 1.2,
            Vec2::new(20.0, 20.0),
            PHYSICAL,
            1.0,
            ColorU8::rgba(240, 240, 240, 255),
        )];
        shaper.drop_cosmic_buffers();
        assert!(
            !shaper.has_cosmic_buffer(runs[0].text.key),
            "fixture must start with an evicted shaped buffer",
        );

        // Frame 1: both caches miss, so the backend reconstructs the shaped
        // buffer before rasterizing and caching the encoded glyphs.
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            2.0,
            &runs,
        );
        assert!(
            shaper.has_cosmic_buffer(runs[0].text.key),
            "an encoded-cache miss must restore its shaped buffer",
        );
        let arena_after_warmup = backend.encoder.cache.arena.len();
        backend.tick_frame();
        assert!(
            !backend.encoder.atlas.cache.is_empty(),
            "warmup should have rasterized at least one glyph",
        );

        // Frame 2: same run → encoded-cache hit (no cosmic walk, no new
        // rasterization). The hit must still bump every slot's
        // last_use to the now-current frame.
        let shaper_borrow = shaper.hold_borrow();
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            2.0,
            &runs,
        );
        drop(shaper_borrow);

        let cf = backend.encoder.atlas.current_frame;
        let stale: Vec<u64> = backend
            .encoder
            .atlas
            .cache
            .values()
            .map(|&i| backend.encoder.atlas.slots[i as usize].last_use)
            .filter(|&lu| lu != cf)
            .collect();
        assert!(
            stale.is_empty(),
            "cache-hit frame left slots stale: last_use {stale:?} != current_frame {cf}",
        );
        // The refresh must have gone through the entry's *recorded*
        // slab indices — the exact path the hot loop writes.
        for entry in backend.encoder.cache.map.values() {
            for glyph in &backend.encoder.cache.arena[entry.span.range()] {
                let idx = glyph.atlas_slot;
                assert_eq!(
                    backend.encoder.atlas.slots[idx as usize].last_use, cf,
                    "recorded slab index {idx} not refreshed on hit",
                );
            }
        }
        assert_eq!(
            backend.encoder.cache.arena.len(),
            arena_after_warmup,
            "a pure cache-hit frame must not append a replacement span",
        );
    }

    #[test]
    fn slot_generation_invalidates_only_referencing_run() {
        let gpu = test_gpu();
        let shaper = TextShaper::new();
        let store = RecordStore::default();
        let mut backend = TextBackend::new(&gpu.lease.device, shaper.clone());

        let runs = [
            make_inner_run(
                &store,
                &shaper,
                "AB",
                14.0,
                14.0 * 1.2,
                Vec2::new(20.0, 20.0),
                PHYSICAL,
                1.0,
                ColorU8::rgba(240, 240, 240, 255),
            ),
            make_inner_run(
                &store,
                &shaper,
                "ZZZZ",
                14.0,
                14.0 * 1.2,
                Vec2::new(20.0, 60.0),
                PHYSICAL,
                1.0,
                ColorU8::rgba(240, 240, 240, 255),
            ),
        ];

        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            2.0,
            &runs,
        );
        assert_eq!(backend.encoder.cache.map.len(), 2);
        backend.tick_frame();

        let entries: Vec<_> = backend
            .encoder
            .cache
            .map
            .iter()
            .map(|(&key, entry)| (key, entry.span))
            .collect();
        // Invalidate the two-glyph "AB" run through its *second* glyph:
        // the cache-hit replay then validates and emits "A" before the
        // mismatch, pinning the partial-output rollback rather than a
        // first-glyph bail.
        let (invalidated_key, invalidated_span) = entries
            .iter()
            .copied()
            .find(|(_, span)| span.len == 2)
            .expect("the two-glyph run must have a cached span");
        let invalidated_slot = backend.encoder.cache.arena[invalidated_span.range()][1].atlas_slot;
        let (stable_key, stable_span) = entries
            .iter()
            .copied()
            .find(|(_, span)| {
                backend.encoder.cache.arena[span.range()]
                    .iter()
                    .all(|glyph| glyph.atlas_slot != invalidated_slot)
            })
            .expect("test runs must use disjoint atlas slots");
        let arena_before = backend.encoder.cache.arena.len();

        let slot = &mut backend.encoder.atlas.slots[invalidated_slot as usize];
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("test slot generation overflowed");
        let expected_generation = slot.generation;
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            2.0,
            &runs,
        );

        assert_eq!(
            backend.encoder.instances.len(),
            6,
            "the rolled-back hit must not leak its partially emitted glyphs",
        );
        assert_eq!(
            backend.encoder.cache.map[&stable_key].span, stable_span,
            "a disjoint run must retain its encoded span",
        );
        // The rebuild is proven by the generation below, not by where
        // the row landed: dropping the stale row frees its block, and
        // the re-encode is the same length, so it reclaims that very
        // block. Asserting *that* is the stronger statement — an
        // invalidation must not cost arena growth.
        let replacement = backend.encoder.cache.map[&invalidated_key].span;
        assert_eq!(
            replacement, invalidated_span,
            "the rebuilt run must reclaim the block its stale template freed",
        );
        assert_eq!(
            backend.encoder.cache.arena.len(),
            arena_before,
            "a slot invalidation must not grow the arena",
        );
        assert_eq!(
            backend.encoder.cache.arena[replacement.range()][1].generation,
            expected_generation,
            "the replacement must record the slot's new generation",
        );
    }

    /// Two batches prepared in one frame ride a single deferred vbuf
    /// write (`TextBackend::flush` after all `prepare_batch` calls). The
    /// per-batch `ranges` must partition the shared instance vec and
    /// each batch's glyphs must keep their own color/placement — same
    /// text at a different origin/color pins this glyph-by-glyph: same
    /// atlas uv + dim, x identical, y shifted by exactly the origin
    /// delta (40 px, integer so subpixel bins match), colors distinct.
    #[test]
    fn deferred_upload_keeps_batches_distinct() {
        let gpu = test_gpu();
        let shaper = TextShaper::new();
        let store = RecordStore::default();
        let mut backend = TextBackend::new(&gpu.lease.device, shaper.clone());

        let color_a = ColorU8::rgba(240, 240, 240, 255);
        let color_b = ColorU8::rgba(200, 100, 50, 255);
        let run_a = make_inner_run(
            &store,
            &shaper,
            "File",
            14.0,
            16.8,
            Vec2::new(20.0, 20.0),
            PHYSICAL,
            1.0,
            color_a,
        );
        let run_b = make_inner_run(
            &store,
            &shaper,
            "File",
            14.0,
            16.8,
            Vec2::new(20.0, 60.0),
            PHYSICAL,
            1.0,
            color_b,
        );

        let mut belt = StagingBelt::new(gpu.lease.device.clone(), 1 << 16);
        let mut encoder = gpu
            .lease
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut ctx = GpuCtx::new(&gpu.lease.device, &gpu.queue, &mut belt, &mut encoder);
            let payloads = store.payloads.borrow();
            let interned_text = payloads.interned_text();
            backend.prepare_batch(
                &mut ctx,
                1.0,
                0,
                std::slice::from_ref(&run_a),
                &interned_text,
            );
            backend.prepare_batch(
                &mut ctx,
                1.0,
                1,
                std::slice::from_ref(&run_b),
                &interned_text,
            );
            backend.flush(&mut ctx);
        }
        belt.finish_and_recall_on_submit(&encoder);
        gpu.queue.submit([encoder.finish()]);

        // Same text → same glyph count n per batch; ranges partition
        // the vec as [0..n] + [n..2n].
        let n = backend.encoder.instances.len() / 2;
        assert!(n > 0, "'File' must emit glyphs");
        assert_eq!(backend.ranges[0], Span::new(0, n as u32));
        assert_eq!(backend.ranges[1], Span::new(n as u32, n as u32));

        let a: u32 = bytemuck::cast(color_a);
        let b: u32 = bytemuck::cast(color_b);
        for (ga, gb) in backend.encoder.instances[..n]
            .iter()
            .zip(&backend.encoder.instances[n..2 * n])
        {
            assert_eq!(ga.color, a);
            assert_eq!(gb.color, b);
            // Identical glyph, identical atlas slot, shifted 40 px down.
            assert_eq!(gb.uv_and_kind, ga.uv_and_kind);
            assert_eq!(gb.dim, ga.dim);
            assert_eq!(gb.pos, [ga.pos[0], ga.pos[1] + 40]);
        }
        backend.tick_frame();
    }

    /// A run whose lines are partially y-culled by its bounds must not
    /// populate the encoded cache: `EncodedKey` omits bounds, so after
    /// integer-pixel scrolling the same key would replay the truncated
    /// template and newly revealed lines would stay blank forever.
    #[test]
    fn partially_culled_run_is_not_cached() {
        let gpu = test_gpu();
        let shaper = TextShaper::new();
        let store = RecordStore::default();
        let mut backend = TextBackend::new(&gpu.lease.device, shaper.clone());

        // Three 3-glyph lines at line_height 16 px, origin (0, 0):
        // line tops sit at 0 / 16 / 32.
        let mut run = make_inner_run(
            &store,
            &shaper,
            "abc\ndef\nxyz",
            14.0,
            16.0,
            Vec2::ZERO,
            PHYSICAL,
            1.0,
            ColorU8::rgba(240, 240, 240, 255),
        );
        // Clip to the first line: the pre-cull keeps lines with
        // line_top <= bounds_bot, so h = 10 keeps line 0 (top 0) and
        // drops lines 1-2 (tops 16, 32).
        run.bounds = URect::new(0, 0, PHYSICAL.x, 10);

        // Frame 1: clipped encode → 1 line * 3 glyphs = 3 instances,
        // and no cache entry.
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            1.0,
            std::slice::from_ref(&run),
        );
        assert_eq!(
            backend.encoder.instances.len(),
            3,
            "only line 0's 3 glyphs survive the cull"
        );
        assert!(
            backend.encoder.cache.map.is_empty(),
            "a culled encode must not become a cache template",
        );
        backend.tick_frame();

        // Frame 2, same clipped run: still a miss, re-encodes to the
        // same 3 instances, still nothing cached.
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            1.0,
            std::slice::from_ref(&run),
        );
        assert_eq!(backend.encoder.instances.len(), 3);
        assert!(backend.encoder.cache.map.is_empty());
        backend.tick_frame();

        // Frame 3, unclipped: 3 lines * 3 glyphs = 9 instances, and
        // the full encode is cached (same key as the clipped frames —
        // that's exactly why the clipped ones must not insert).
        run.bounds = URect::new(0, 0, PHYSICAL.x, PHYSICAL.y);
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            1.0,
            std::slice::from_ref(&run),
        );
        assert_eq!(backend.encoder.instances.len(), 9);
        assert_eq!(backend.encoder.cache.map.len(), 1);
        let cached = backend.encoder.cache.map.values().next().unwrap().span;
        assert_eq!(
            cached.len, 9,
            "the whole run is cached, not a culled prefix"
        );
        // Blocks round up to `BLOCK_GRANULE`, so nine glyphs occupy a
        // twelve-slot block. The row's own length is the invariant here;
        // the arena length is the allocator's business.
        assert_eq!(backend.encoder.cache.arena.len(), 12);
        backend.tick_frame();

        // Frame 4 replays the cached template: same 9 instances with
        // no re-encode (the arena didn't grow).
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            1.0,
            std::slice::from_ref(&run),
        );
        assert_eq!(backend.encoder.instances.len(), 9);
        assert_eq!(backend.encoder.cache.map.len(), 1);
        assert_eq!(
            backend.encoder.cache.arena.len(),
            12,
            "a hit must not re-encode"
        );
    }

    /// Both text caches age on one clock, so a frame that draws no text
    /// ages neither — and one that draws text ages both by the same
    /// step.
    ///
    /// The regression: this side used to run its own counter, bumped in
    /// `end_frame`, which returned early whenever the frame prepared
    /// no text batch. A recorded frame whose damage missed every text
    /// run therefore aged the shaped-buffer cache and not this one, so
    /// `RENDERED_RUN_KEEP_FRAMES` — one constant precisely so a buffer
    /// outlives the encoded entry that would come asking for it —
    /// described two windows measured in different units. Nothing could
    /// catch it: each suite drove one clock.
    #[test]
    fn both_caches_age_on_one_clock_including_text_free_frames() {
        let gpu = test_gpu();
        let shaper = TextShaper::new();
        let store = RecordStore::default();
        let mut backend = TextBackend::new(&gpu.lease.device, shaper.clone());

        let runs = [make_inner_run(
            &store,
            &shaper,
            "aged",
            14.0,
            14.0 * 1.2,
            Vec2::new(20.0, 20.0),
            PHYSICAL,
            1.0,
            ColorU8::rgba(240, 240, 240, 255),
        )];
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            1.0,
            &runs,
        );
        assert_eq!(backend.encoder.cache.map.len(), 1, "the run is cached");
        backend.tick_frame();

        // Text-free frames: `prepare_batch` is never called, so `ranges`
        // stays empty — the exact shape that used to freeze this side.
        // Both clocks must still move, in lockstep.
        for _ in 0..8 {
            let before = shaper.frame();
            backend.tick_frame();
            assert_eq!(
                shaper.frame(),
                before + 1,
                "a text-free frame must still advance the shared clock",
            );
            assert_eq!(
                backend.encoder.atlas.current_frame,
                shaper.frame(),
                "the atlas must track the shaper's clock, not its own count",
            );
        }

        // And the encoded entry expires on that same clock: it was last
        // used at frame 0, so it dies one frame past its window, without
        // a single text-bearing frame in between.
        assert_eq!(
            backend.encoder.cache.map.len(),
            1,
            "premise: still inside the keep window",
        );
        while shaper.frame() <= crate::text::RENDERED_RUN_KEEP_FRAMES {
            backend.tick_frame();
        }
        assert!(
            backend.encoder.cache.map.is_empty(),
            "text-free frames must age the encoded cache out",
        );
        assert!(
            !shaper.has_cosmic_buffer(runs[0].text.key),
            "…and the shaped buffer with it, on the same clock",
        );
    }

    /// A zero-area glyph entry (whitespace) swept by the periodic
    /// empty-entry sweep must re-insert cleanly through `insert_unallocated`
    /// on next use.
    #[test]
    fn swept_empty_glyph_reinserts() {
        let gpu = test_gpu();
        let shaper = TextShaper::new();
        let store = RecordStore::default();
        let mut backend = TextBackend::new(&gpu.lease.device, shaper.clone());

        let runs = [make_inner_run(
            &store,
            &shaper,
            " ",
            14.0,
            16.0,
            Vec2::new(2.0, 2.0),
            PHYSICAL,
            1.0,
            ColorU8::rgba(240, 240, 240, 255),
        )];
        let empties = |b: &TextBackend| {
            b.encoder
                .atlas
                .cache
                .values()
                .filter(|&&i| b.encoder.atlas.slots[i as usize].alloc.is_none())
                .count()
        };

        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            1.0,
            &runs,
        );
        assert!(
            backend.encoder.instances.is_empty(),
            "whitespace prepares a text batch without drawable glyphs",
        );
        assert_eq!(
            empties(&backend),
            1,
            "the space rasterizes to one zero-area entry"
        );
        let first_frame = backend.encoder.atlas.current_frame;
        backend.tick_frame();
        assert_eq!(
            backend.encoder.atlas.current_frame,
            first_frame + 1,
            "a prepared zero-instance batch must still advance cache aging",
        );

        // The space was rasterized on frame 0, so its last_use is 0 and
        // its ticket falls due at 0 + 512 + 1 = 513 — its own deadline,
        // not a shared tick. Advance well past that with prepared text
        // frames that never touch the space again.
        let mut belt = StagingBelt::new(gpu.lease.device.clone(), 1 << 16);
        let mut encoder = gpu
            .lease
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let payloads = store.payloads.borrow();
        let interned_text = payloads.interned_text();
        while backend.encoder.atlas.current_frame < 1024 {
            let mut ctx = GpuCtx::new(&gpu.lease.device, &gpu.queue, &mut belt, &mut encoder);
            backend.prepare_batch(&mut ctx, 1.0, 0, &[], &interned_text);
            backend.tick_frame();
        }
        assert_eq!(
            empties(&backend),
            0,
            "stale empty entry reclaimed once its own window lapsed",
        );

        // Re-encoding the same run re-inserts the empty entry (the
        // encoded cache was itself swept after 120 idle frames, so this
        // is a full walk through rasterize_and_insert → insert_unallocated).
        run_one_frame(
            &gpu.lease.device,
            &gpu.queue,
            &mut backend,
            &store,
            1.0,
            &runs,
        );
        assert_eq!(
            empties(&backend),
            1,
            "swept empty glyph re-inserts on next use"
        );
        backend.tick_frame();
    }
}
