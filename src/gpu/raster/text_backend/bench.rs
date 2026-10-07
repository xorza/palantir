//! Text-backend microbench: `prepare`, `flush` and `render_batch` driven directly against `TextBackend`, bypassing `WindowDriver`.
//!
//! - `text_atlas/steady_warm`: fixed scale, atlas primed; every glyph is an `atlas.touch` hit.
//! - `zoom_smooth` / `zoom_cold`: five resident scale rungs in adjacent or jumping order (encoded-cache hits).
//! - `cache_churn`: 128 rungs in permuted order, past the retention window, so each revisit misses.
//! - `mixed_stable_churn`: half the runs stay at one hot scale while the rest cycle the cold rungs, to see whether atlas pressure rebuilds unrelated stable runs.
//!
//! Run with `cargo bench --bench gpu --features bench -- text_atlas`.

#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::common::counters::CounterSet;
use std::sync::OnceLock;
use std::time::Duration;

use crate::bench::Run;
use crate::gpu::bench_gpu::{BenchGpu, TARGET_FORMAT, Timing};
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::raster::raster_program::RasterProgram;
use crate::gpu::raster::text_backend::TextBackend;
use crate::gpu::raster::text_backend::encode::cache::internals::{ChurnBench, SweepBench};
use crate::gpu::surface::viewport::ViewportPush;
use crate::primitives::geometry::urect::URect;
use crate::primitives::layout::align::Align;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::text::interned_text::InternedText;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::scene::record_store::RecordStore;
use crate::text::glyph_font::GlyphFont;
use crate::text::run::TextRun;
use crate::text::shaped_ref::ShapedTextRef;
use crate::text::shaper::TextShaper;
use crate::text::wrap::TextWrap;
use crate::text::{RENDERED_RUN_KEEP_FRAMES, TEXT_SCALE_STEP};
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use std::slice;
use wgpu::util::StagingBelt;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const BASE_SCALE: f32 = 2.0;
const WARM_SCALE_CYCLE: u32 = 5;

/// Rungs the churn arms cycle through, enough to put the mask atlas past `RasterAtlasConfig::eager_growth_bytes`, where it recycles rectangles (`RasterAtlas::evict_one`); eviction begins between rung 250 and 500. The only arm coverage of that regime; `report_atlas_pressure` prints which side an arm landed on.
const CHURN_SCALE_CYCLE: u32 = 512;
const CHURN_INDEX_STRIDE: u32 = 37;

const ROWS: u32 = 32;

/// Distinct-text run count for the large-stable-key-set workload: the steady scene's runs collapse onto few cache rows (`EncodedKey` folds in only text key, quantized scale, colour and subpixel bins), and maintenance scales with rows.
const DISTINCT_RUNS: usize = 512;

#[derive(Debug)]
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

#[derive(Debug)]
struct BenchText {
    backend: TextBackend,
    pipelines: StencilVariant,
}

#[derive(Clone, Copy, Debug)]
struct BenchBatch<'a> {
    runs: &'a [TextDrawRow],
    scale: f32,
}

#[derive(Debug)]
struct BenchRuns {
    store: RecordStore,
    runs: Vec<TextDrawRow>,
}

impl BenchText {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, shaper: TextShaper) -> Self {
        let raster = RasterProgram::new(device);
        let backend = TextBackend::new(device, &raster, shaper);
        let pipelines = raster.build_variants(device, format);
        Self { backend, pipelines }
    }

    fn prepare(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        scale: f32,
        runs: &[TextDrawRow],
        interned_text: &InternedText<'_>,
    ) {
        self.prepare_batch(ctx, scale, 0, runs, interned_text);
    }

    fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        scale: f32,
        batch_index: usize,
        runs: &[TextDrawRow],
        interned_text: &InternedText<'_>,
    ) {
        self.backend
            .prepare_batch(ctx, scale, batch_index, runs, interned_text);
    }

    fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
        self.backend.flush(ctx);
    }

    /// `render_batch` binds neither pipeline nor viewport (the render loop owns both, see `Bound::Raster`), so a standalone pass binds its own.
    fn draw<'a>(&'a self, batch_index: usize, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(self.pipelines.select(false));
        ViewportPush { size: Vec2::ZERO }.push_into(pass);
        self.backend.render_batch(batch_index, pass);
    }

    fn end_frame(&mut self) {
        self.backend.tick_frame();
    }
}

fn gpu() -> &'static Gpu {
    static G: OnceLock<Gpu> = OnceLock::new();
    G.get_or_init(|| {
        let shared = BenchGpu::shared(Timing::Bare);
        Gpu {
            device: shared.gpu.device.clone(),
            queue: shared.gpu.queue.clone(),
        }
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "a fixture builder that mirrors the call under test rather than grouping"
)]
fn make_run(
    store: &mut RecordStore,
    shaper: &TextShaper,
    text: &str,
    font_size: f32,
    line_height: f32,
    origin: Vec2,
    viewport: UVec2,
    scale: f32,
    color: RgbaF16,
) -> TextDrawRow {
    let interned = store.intern(text);
    let recorded = store.record_text(interned);
    // Warm through the run so the stamped key is the one the shaped buffer landed under (no width, non-binding policy: the unbounded root).
    let run = TextRun {
        text,
        font: GlyphFont {
            line_height,
            ..GlyphFont::new(font_size)
        },
        wrap: TextWrap::SingleLine,
        align: Align::default(),
        max_width: None,
    };
    let key = run
        .unbounded_key()
        .expect("the bench fixture names a usable face");
    shaper.layout(&run);
    TextDrawRow {
        text: ShapedTextRef::new(key, &recorded),
        origin,
        bounds: URect::new(0, 0, viewport.x, viewport.y),
        color,
        scale,
    }
}

/// Shapes one frame's runs against `shaper`; the slice is reused and only `scale` changes.
fn build_runs(shaper: &TextShaper) -> BenchRuns {
    let mut store = RecordStore::default();
    let color = RgbaF16::new(0.86, 0.86, 0.86, 1.0);
    let mut runs = Vec::with_capacity((ROWS * 4) as usize);
    for row in 0..ROWS {
        let y = 16.0 + (row as f32) * 18.0;
        let label_color = RgbaF16::new(0.96, 0.96, 0.96, 1.0);
        runs.push(make_run(
            &mut store,
            shaper,
            "node",
            13.0,
            13.0 * 1.2,
            Vec2::new(16.0, y),
            PHYSICAL,
            1.0,
            label_color,
        ));
        runs.push(make_run(
            &mut store,
            shaper,
            "input: f32",
            11.0,
            11.0 * 1.2,
            Vec2::new(80.0, y),
            PHYSICAL,
            1.0,
            color,
        ));
        runs.push(make_run(
            &mut store,
            shaper,
            "output: Vec3",
            11.0,
            11.0 * 1.2,
            Vec2::new(220.0, y),
            PHYSICAL,
            1.0,
            color,
        ));
        runs.push(make_run(
            &mut store,
            shaper,
            "123.45",
            11.0,
            11.0 * 1.2,
            Vec2::new(380.0, y),
            PHYSICAL,
            1.0,
            color,
        ));
    }
    BenchRuns { store, runs }
}

fn run_frame(
    g: &Gpu,
    backend: &mut BenchText,
    belt: &mut StagingBelt,
    target_view: &wgpu::TextureView,
    store: &RecordStore,
    runs: &[TextDrawRow],
    scale: f32,
) {
    run_batches(
        g,
        backend,
        belt,
        target_view,
        store,
        slice::from_ref(&BenchBatch { runs, scale }),
    );
}

fn run_batches(
    g: &Gpu,
    backend: &mut BenchText,
    belt: &mut StagingBelt,
    target_view: &wgpu::TextureView,
    store: &RecordStore,
    batches: &[BenchBatch<'_>],
) {
    let mut encoder = g
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("palantir.text_atlas.encoder"),
        });
    {
        let mut ctx = GpuCtx::new(&g.device, &g.queue, belt, &mut encoder);
        let interned_text = store.interned_text();
        for (batch_index, batch) in batches.iter().enumerate() {
            backend.prepare_batch(
                &mut ctx,
                batch.scale,
                batch_index,
                batch.runs,
                &interned_text,
            );
        }
        backend.flush(&mut ctx);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.text_atlas.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        for batch_index in 0..batches.len() {
            backend.draw(batch_index, &mut pass);
        }
    }
    belt.finish();
    g.queue.submit([encoder.finish()]);
    belt.recall();
    BenchGpu::shared(Timing::Bare).wait();
    backend.end_frame();
}

/// [`DISTINCT_RUNS`] runs with distinct texts, each in its own encoded-cache row, at integral origins with y inside the viewport (a y-culled run isn't cached).
fn build_distinct_runs(shaper: &TextShaper) -> BenchRuns {
    let mut store = RecordStore::default();
    let color = RgbaF16::new(0.86, 0.86, 0.86, 1.0);
    let mut runs = Vec::with_capacity(DISTINCT_RUNS);
    for i in 0..DISTINCT_RUNS {
        let text = format!("field {i}: f32");
        let row = i as u32 % ROWS;
        let column = i as u32 / ROWS;
        runs.push(make_run(
            &mut store,
            shaper,
            &text,
            11.0,
            11.0 * 1.2,
            Vec2::new(16.0 + (column as f32) * 80.0, 16.0 + (row as f32) * 18.0),
            PHYSICAL,
            1.0,
            color,
        ));
    }
    BenchRuns { store, runs }
}

fn fresh_backend(g: &Gpu) -> (BenchText, BenchRuns) {
    let shaper = TextShaper::new();
    let runs = build_runs(&shaper);
    let backend = BenchText::new(&g.device, TARGET_FORMAT, shaper);
    let _ = PHYSICAL;
    (backend, runs)
}

/// Reports what the glyph atlas paid to stay packed over `frames` primed frames. Printed before the measured section so it doesn't depend on criterion's iteration count or `--list`.
fn report_atlas_pressure(label: &str, backend: &BenchText, frames: u32) {
    let atlas = &backend.backend.pass.atlas;
    let counts = atlas.counters.counts();
    let per_frame = counts.evict_scans as f64 / f64::from(frames.max(1));
    eprintln!(
        "[text_atlas] {label}: live_glyphs={} evictions={} grows={} \
         scanned={} ({per_frame:.0}/frame over {frames} frames) oversized={}",
        atlas.cache.len(),
        counts.evictions,
        counts.grows,
        counts.evict_scans,
        counts.oversized,
    );
    // An entry past the ceiling is refused every frame, so a fixture producing one measures a permanently degraded atlas.
    assert_eq!(
        counts.oversized, 0,
        "[text_atlas] {label}: a glyph exceeded the atlas ceiling — \
         this arm no longer measures what it claims to",
    );
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let g = gpu();
    let target = BenchGpu::shared(Timing::Bare).target("palantir.text_atlas.target", PHYSICAL);
    let view = target.view();

    let mut group = run.group(c);
    group.measurement_time(Duration::from_secs(5));

    {
        let (mut backend, scene) = fresh_backend(g);
        let mut belt = StagingBelt::new(g.device.clone(), 1 << 20);
        for _ in 0..2 {
            run_frame(
                g,
                &mut backend,
                &mut belt,
                &view,
                &scene.store,
                &scene.runs,
                BASE_SCALE,
            );
        }
        group.bench_function("steady_warm", |b| {
            b.iter(|| {
                run_frame(
                    g,
                    &mut backend,
                    &mut belt,
                    &view,
                    &scene.store,
                    &scene.runs,
                    BASE_SCALE,
                );
            });
        });
        // CPU-only, isolating CPU cost from GPU sync; a belt and throwaway encoder satisfy `prepare`'s signature.
        group.bench_function("steady_warm_cpu", |b| {
            b.iter(|| {
                let mut encoder =
                    g.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("palantir.text_atlas.cpu_prepare"),
                        });
                {
                    let mut ctx = GpuCtx::new(&g.device, &g.queue, &mut belt, &mut encoder);
                    let store = &scene.store;
                    let interned_text = store.interned_text();
                    backend.prepare(&mut ctx, BASE_SCALE, &scene.runs, &interned_text);
                }
                belt.finish();
                belt.recall();
                backend.end_frame();
            });
        });
    }

    {
        let (mut backend, scene) = fresh_backend(g);
        let mut belt = StagingBelt::new(g.device.clone(), 1 << 20);
        for step in 0..WARM_SCALE_CYCLE {
            let scale = BASE_SCALE + (step as f32) * TEXT_SCALE_STEP;
            run_frame(
                g,
                &mut backend,
                &mut belt,
                &view,
                &scene.store,
                &scene.runs,
                scale,
            );
        }
        let mut i: u32 = 0;
        group.bench_function("zoom_smooth", |b| {
            b.iter(|| {
                let step = (i % WARM_SCALE_CYCLE) as f32;
                let scale = BASE_SCALE + step * TEXT_SCALE_STEP;
                run_frame(
                    g,
                    &mut backend,
                    &mut belt,
                    &view,
                    &scene.store,
                    &scene.runs,
                    scale,
                );
                i = i.wrapping_add(1);
            });
        });
    }

    {
        let (mut backend, scene) = fresh_backend(g);
        let mut belt = StagingBelt::new(g.device.clone(), 1 << 20);
        let stride = 5.0 * TEXT_SCALE_STEP;
        for step in 0..WARM_SCALE_CYCLE {
            let scale = BASE_SCALE + (step as f32) * stride;
            run_frame(
                g,
                &mut backend,
                &mut belt,
                &view,
                &scene.store,
                &scene.runs,
                scale,
            );
        }
        report_atlas_pressure("zoom_cold", &backend, WARM_SCALE_CYCLE);
        let mut i: u32 = 0;
        group.bench_function("zoom_cold", |b| {
            b.iter(|| {
                let step = (i % WARM_SCALE_CYCLE) as f32;
                let scale = BASE_SCALE + step * stride;
                run_frame(
                    g,
                    &mut backend,
                    &mut belt,
                    &view,
                    &scene.store,
                    &scene.runs,
                    scale,
                );
                i = i.wrapping_add(1);
            });
        });
    }

    {
        let (mut backend, scene) = fresh_backend(g);
        let mut belt = StagingBelt::new(g.device.clone(), 1 << 20);
        for step in 0..CHURN_SCALE_CYCLE {
            let scale = BASE_SCALE + (step as f32) * TEXT_SCALE_STEP;
            run_frame(
                g,
                &mut backend,
                &mut belt,
                &view,
                &scene.store,
                &scene.runs,
                scale,
            );
        }
        report_atlas_pressure("cache_churn", &backend, CHURN_SCALE_CYCLE);
        let mut i: u32 = 0;
        group.bench_function("cache_churn", |b| {
            b.iter(|| {
                let rung = i.wrapping_mul(CHURN_INDEX_STRIDE) % CHURN_SCALE_CYCLE;
                let scale = BASE_SCALE + (rung as f32) * TEXT_SCALE_STEP;
                run_frame(
                    g,
                    &mut backend,
                    &mut belt,
                    &view,
                    &scene.store,
                    &scene.runs,
                    scale,
                );
                i = i.wrapping_add(1);
            });
        });
    }

    {
        let (mut backend, scene) = fresh_backend(g);
        let (stable_runs, churning_runs) = scene.runs.split_at(scene.runs.len() / 2);
        let mut belt = StagingBelt::new(g.device.clone(), 1 << 20);
        for step in 0..CHURN_SCALE_CYCLE {
            let scale = BASE_SCALE + (step as f32) * TEXT_SCALE_STEP;
            run_batches(
                g,
                &mut backend,
                &mut belt,
                &view,
                &scene.store,
                &[
                    BenchBatch {
                        runs: stable_runs,
                        scale: BASE_SCALE,
                    },
                    BenchBatch {
                        runs: churning_runs,
                        scale,
                    },
                ],
            );
        }
        let mut i: u32 = 0;
        group.bench_function("mixed_stable_churn", |b| {
            b.iter(|| {
                let rung = i.wrapping_mul(CHURN_INDEX_STRIDE) % CHURN_SCALE_CYCLE;
                let scale = BASE_SCALE + (rung as f32) * TEXT_SCALE_STEP;
                run_batches(
                    g,
                    &mut backend,
                    &mut belt,
                    &view,
                    &scene.store,
                    &[
                        BenchBatch {
                            runs: stable_runs,
                            scale: BASE_SCALE,
                        },
                        BenchBatch {
                            runs: churning_runs,
                            scale,
                        },
                    ],
                );
                i = i.wrapping_add(1);
            });
        });
    }

    {
        // Large stable key set: every run hits its own row, nothing expires or re-encodes. CPU-only, since a whole-map scan is microseconds against a GPU submit.
        let shaper = TextShaper::new();
        let scene = build_distinct_runs(&shaper);
        let mut backend = BenchText::new(&g.device, TARGET_FORMAT, shaper);
        let mut belt = StagingBelt::new(g.device.clone(), 1 << 20);
        for _ in 0..2 {
            run_frame(
                g,
                &mut backend,
                &mut belt,
                &view,
                &scene.store,
                &scene.runs,
                BASE_SCALE,
            );
        }
        group.bench_function("stable_keys_cpu", |b| {
            b.iter(|| {
                let mut encoder =
                    g.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("palantir.text_atlas.stable_keys_cpu"),
                        });
                {
                    let mut ctx = GpuCtx::new(&g.device, &g.queue, &mut belt, &mut encoder);
                    let store = &scene.store;
                    let interned_text = store.interned_text();
                    backend.prepare(&mut ctx, BASE_SCALE, &scene.runs, &interned_text);
                }
                belt.finish();
                belt.recall();
                backend.end_frame();
            });
        });
    }

    {
        // Counter-workload to `stable_keys_cpu`: every frame lands on a new scale, so every run misses and inserts a row. CPU-only because `cache_churn` (~1 ms, GPU-bound) can't resolve the maintenance tradeoff.
        let (mut backend, scene) = fresh_backend(g);
        let mut belt = StagingBelt::new(g.device.clone(), 1 << 20);
        for step in 0..CHURN_SCALE_CYCLE {
            let scale = BASE_SCALE + (step as f32) * TEXT_SCALE_STEP;
            run_frame(
                g,
                &mut backend,
                &mut belt,
                &view,
                &scene.store,
                &scene.runs,
                scale,
            );
        }
        let mut i: u32 = 0;
        group.bench_function("churn_cpu", |b| {
            b.iter(|| {
                let rung = i.wrapping_mul(CHURN_INDEX_STRIDE) % CHURN_SCALE_CYCLE;
                let scale = BASE_SCALE + (rung as f32) * TEXT_SCALE_STEP;
                let mut encoder =
                    g.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("palantir.text_atlas.churn_cpu"),
                        });
                {
                    let mut ctx = GpuCtx::new(&g.device, &g.queue, &mut belt, &mut encoder);
                    let store = &scene.store;
                    let interned_text = store.interned_text();
                    backend.prepare(&mut ctx, scale, &scene.runs, &interned_text);
                }
                belt.finish();
                belt.recall();
                backend.end_frame();
                i = i.wrapping_add(1);
            });
        });
    }

    group.finish();

    bench_encoded_cache(c, run);
}

/// The encoded cache's per-frame maintenance in two steady states, CPU-only (the `text_atlas` arms can't resolve it under drift).
///
/// - **`steady`**: nothing expires, so every fired ticket re-files; this drain path runs every frame by design (a cadence gate would trade uniform cost for a spike), so it must stay small. 12 glyphs per row matches the 512-row scene (~11.8).
/// - **`churn`**: a zoom or width drag re-keys every run; the only arm running `settle`'s allocate-and-copy.
///
/// **Neither guards uniformity:** the replaced compaction was amortised free (one frame in 122 paying 122x), so the defect lived in the tail. `a_saturated_gesture_reaches_a_steady_state_where_no_frame_allocates` guards the shape; these guard the constant.
fn bench_encoded_cache(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.subgroup(c, "encoded_cache");
    group.measurement_time(Duration::from_secs(2));

    for rows in [128u32, 512] {
        let mut fixture = SweepBench::new(rows, 12);
        assert_eq!(fixture.sweep_steady(), rows as usize);
        group.bench_with_input(BenchmarkId::new("steady", rows), &rows, |b, _| {
            b.iter(|| black_box(fixture.sweep_steady()));
        });
    }

    // Two sizes: per-glyph cost is flat once fixed overhead stops dominating (413 Melem/s at 50x25 against 431 at 200x40); the large one is the 6.8 MB-arena shape where cache pressure could show.
    //
    // Warmed past the retention window (against `RENDERED_RUN_KEEP_FRAMES`, the documented ceiling), since the arena is still growing before saturation.
    for (runs, glyphs) in [(8u32, 12u32), (200, 40)] {
        let mut fixture = ChurnBench::new(runs, glyphs);
        for _ in 0..RENDERED_RUN_KEEP_FRAMES * 2 {
            fixture.churn_frame();
        }
        let saturated = fixture.arena_len();
        group.throughput(Throughput::Elements(u64::from(runs * glyphs)));
        group.bench_with_input(
            BenchmarkId::new("churn", format!("{runs}x{glyphs}")),
            &runs,
            |b, _| b.iter(|| black_box(fixture.churn_frame())),
        );
        // A saturated gesture recycles, so the arena must not have grown; a regression invalidates the number.
        assert_eq!(
            fixture.arena_len(),
            saturated,
            "{runs}x{glyphs}: the measured frames grew the arena",
        );
    }

    group.finish();
}
