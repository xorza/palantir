//! Text-backend microbench: prepare + flush + render directly against
//! `TextBackend`, bypassing the full `WindowDriver` pipeline.
//!
//! The previous version drove the full offscreen host frame, which mixed
//! record/measure/cascade/encode noise into every sample —
//! `CascadeEngine::run` was the top hotspot at ~7%, and the actual
//! text path (`encode_batch` + atlas uploads) totalled <10%. This
//! bench skips all of that: a fixed slice of `TextDrawRow`s, shaped once
//! at construction, fed into `TextBackend::prepare` →
//! `flush` → `render_batch` each iteration.
//!
//! Two motivating workloads:
//!
//! - `text_atlas/steady_warm` — fixed scale, atlas warmed by two
//!   priming iterations. Every glyph is an `atlas.touch` hit; the
//!   measurement floor is `encode_batch` walking layout runs +
//!   `swash_cache::CacheKey::new` + vertex buffer upload + draw.
//! - `text_atlas/zoom_smooth` / `zoom_cold` — cycle through five
//!   resident scale rungs in adjacent or jumping order. These isolate
//!   multi-scale encoded-cache hits after all rungs are primed.
//! - `text_atlas/cache_churn` — cycles through 128 scale rungs in a
//!   permuted order. That exceeds the encoded-cache retention window,
//!   so every revisited rung is a real encode/raster/upload miss.
//! - `text_atlas/mixed_stable_churn` — keeps half the runs at one hot
//!   scale while the other half cycles through those 128 cold rungs.
//!   This isolates whether atlas pressure rebuilds unrelated stable
//!   encoded runs.
//!
//! Each iteration:
//!   1. begin command encoder
//!   2. `prepare` (shape lookup + encode_batch into instance Vec +
//!      potential atlas grow + vbuf upload + params reupload)
//!   3. `flush` (upload instances + drain pending glyph uploads into
//!      encoder)
//!   4. render pass: `render_batch` → submit → `poll(Wait)` so the
//!      GPU work drains before the next iteration.
//!   5. `end_frame` (atlas trim + clear instance Vec + reset ranges)
//!
//! Run with:
//!   cargo bench --bench gpu --features bench -- text_atlas
//!   cargo bench --bench gpu --features bench -- 'zoom_smooth$'

use std::sync::OnceLock;
use std::time::Duration;

use crate::host::bench_gpu::{BenchGpu, TARGET_FORMAT, Timing};
use crate::primitives::color::ColorU8;
use crate::primitives::interned_str::InternedText;
use crate::primitives::urect::URect;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::StencilVariant;
use crate::renderer::backend::queue::Queue;
use crate::renderer::backend::text::TextBackend;
use crate::renderer::backend::text::encode::internals::{ChurnBench, SweepBench};
use crate::renderer::backend::viewport::ViewportPush;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::scene::record_store::RecordStore;
use crate::text::RENDERED_RUN_KEEP_FRAMES;
use crate::text::request::TextShapeRequest;
use crate::text::shaped_ref::ShapedTextRef;
use crate::text::shaper::TextShaper;
use crate::text::{FontFamily, FontWeight};
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use wgpu::util::StagingBelt;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const BASE_SCALE: f32 = 2.0;
/// Matches `crate::text::TEXT_SCALE_STEP`, the ladder the composer
/// actually snaps a zoom to. It used to be 0.025 here — 5x coarser, so
/// a given zoom range minted a fifth of the rungs and the churn arms
/// modelled a gentler gesture than any real one.
const TEXT_SCALE_STEP: f32 = crate::text::TEXT_SCALE_STEP;
const WARM_SCALE_CYCLE: u32 = 5;

/// Rungs the churn arms cycle through.
///
/// Sized to put the mask atlas **past** `EAGER_GROWTH_BYTE_BUDGET` — the
/// point where it stops growing and starts recycling rectangles, which
/// is where `GlyphAtlas::evict_one` bills. Measured on this fixture, the
/// side fills and eviction begins between rung 250 and 500; 512 keeps
/// the benched iterations solidly on the far side of that.
///
/// This is the crate's only coverage of that regime. Every other
/// `text_atlas` arm sits below it — `zoom_cold` peaks at 137 live
/// glyphs and the pre-widening `cache_churn` at 3700, both with *zero*
/// evictions — so a change to the eviction policy used to be
/// unmeasurable here. `report_atlas_pressure` prints which side of the
/// line an arm landed on, so a future retune can tell at a glance.
const CHURN_SCALE_CYCLE: u32 = 512;
/// Coprime with [`CHURN_SCALE_CYCLE`], so cycling `i * STRIDE % CYCLE`
/// visits every rung before repeating and the revisit order stays a
/// permutation rather than a short orbit.
const CHURN_INDEX_STRIDE: u32 = 37;

/// Per-frame text count. Graph-view-shaped: many small runs rather
/// than a few wrapped paragraphs. 32 rows × 4 columns = 128 runs ≈
/// what the showcase's node graph tab paints.
const ROWS: u32 = 32;

/// Distinct-text run count for the large-stable-key-set workload. The
/// steady scene reuses four labels at integral origins, and
/// `EncodedKey` folds in only the text key, quantized scale, colour and
/// subpixel bins — so all 128 of its runs collapse onto a handful of
/// cache rows. Encoded-cache maintenance scales with *rows*, so pinning
/// it needs a scene where every run owns one.
const DISTINCT_RUNS: usize = 512;

#[derive(Debug)]
struct Gpu {
    device: wgpu::Device,
    queue: Queue,
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
        let backend = TextBackend::new(device, shaper);
        let pipelines = backend.build_variants(device, format);
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

    fn draw<'a>(&'a self, batch_index: usize, pass: &mut wgpu::RenderPass<'a>) {
        let viewport = ViewportPush {
            size: glam::Vec2::ZERO,
        };
        self.backend
            .render_batch(batch_index, pass, &self.pipelines, false, &viewport);
    }

    /// Frame teardown for the harness, matching `TextSystem`'s
    /// on the production side: what it drives happens to advance the
    /// shared text clock, but the harness is modelling a frame boundary,
    /// not owning the clock.
    fn end_frame(&mut self) {
        self.backend.tick_frame();
    }
}

fn gpu() -> &'static Gpu {
    static G: OnceLock<Gpu> = OnceLock::new();
    G.get_or_init(|| {
        let shared = BenchGpu::shared(Timing::Bare);
        Gpu {
            device: shared.device.clone(),
            queue: Queue::new(shared.queue.clone()),
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn make_run(
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

/// Shape one frame's worth of runs against `shaper`. Stable layout so
/// the same `TextDrawRow` slice is reusable across iterations; only the
/// per-iteration `scale` argument to `prepare` changes between frames.
fn build_runs(shaper: &TextShaper) -> BenchRuns {
    let store = RecordStore::default();
    let color = ColorU8::rgba(220, 220, 220, 255);
    let mut runs = Vec::with_capacity((ROWS * 4) as usize);
    for row in 0..ROWS {
        let y = 16.0 + (row as f32) * 18.0;
        // Four short labels per row at typical graph-node sizes.
        let label_color = ColorU8::rgba(245, 245, 245, 255);
        runs.push(make_run(
            &store,
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
            &store,
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
            &store,
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
            &store,
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

/// One iteration: prepare → flush → render pass → submit → poll →
/// post. Mirrors `OffscreenHost::frame_offscreen`'s text-relevant slice.
fn run_frame(
    g: &Gpu,
    backend: &mut BenchText,
    belt: &mut wgpu::util::StagingBelt,
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
        std::slice::from_ref(&BenchBatch { runs, scale }),
    );
}

fn run_batches(
    g: &Gpu,
    backend: &mut BenchText,
    belt: &mut wgpu::util::StagingBelt,
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
        let payloads = store.payloads.borrow();
        let interned_text = payloads.interned_text();
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

/// [`DISTINCT_RUNS`] runs whose texts all differ, so each occupies its
/// own encoded-cache row. Laid out in `ROWS` columns-worth of rows with
/// every origin integral: y stays inside the viewport so no run is
/// y-culled (a culled run is deliberately not cached, which would leave
/// the map empty and defeat the workload).
fn build_distinct_runs(shaper: &TextShaper) -> BenchRuns {
    let store = RecordStore::default();
    let color = ColorU8::rgba(220, 220, 220, 255);
    let mut runs = Vec::with_capacity(DISTINCT_RUNS);
    for i in 0..DISTINCT_RUNS {
        let text = format!("field {i}: f32");
        let row = i as u32 % ROWS;
        let column = i as u32 / ROWS;
        runs.push(make_run(
            &store,
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
    // Viewport is no longer the text backend's concern — it reads
    // from the shared `@group(0)` uniform the production host binds.
    // The bench's atlas-only fixture doesn't actually issue draws
    // that need it, so leaving it unset is safe.
    let _ = PHYSICAL;
    (backend, runs)
}

/// Report what the glyph atlas paid to stay packed over `frames` primed
/// frames — in particular whether it reached the regime where it
/// recycles rectangles instead of growing, and what the clock's hand
/// cost it there.
///
/// Printed before the measured section, like the residency guard in
/// `text_shape`: reading it afterwards would make the number depend on
/// whatever iteration count criterion chose, and report nothing at all
/// under `--list`.
fn report_atlas_pressure(label: &str, backend: &BenchText, frames: u32) {
    let atlas = &backend.backend.encoder.atlas;
    let counts = atlas.probe.counts();
    let per_frame = counts.evict_scans as f64 / frames.max(1) as f64;
    eprintln!(
        "[text_atlas] {label}: live_glyphs={} evictions={} grows={} \
         scanned={} ({per_frame:.0}/frame over {frames} frames)",
        atlas.cache.len(),
        counts.evictions,
        counts.grows,
        counts.evict_scans,
    );
}

pub(crate) fn bench(c: &mut Criterion, _: crate::bench::Run<'_>) {
    let g = gpu();
    let target = BenchGpu::shared(Timing::Bare).target(PHYSICAL, "palantir.text_atlas.target");
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let mut group = c.benchmark_group("text_atlas");
    group.measurement_time(Duration::from_secs(5));

    {
        let (mut backend, scene) = fresh_backend(g);
        let mut belt = StagingBelt::new(g.device.clone(), 1 << 20);
        // Two priming frames so every glyph is in the atlas.
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
        // CPU-only: prepare + end_frame, no encoder/submit/poll.
        // Isolates text-backend CPU work from GPU sync — useful when
        // the full case looks GPU-bound and you want to see whether a
        // change moved the CPU prepare cost. Still needs a belt +
        // throwaway encoder to satisfy `prepare`'s signature; the
        // encoder is discarded.
        group.bench_function("steady_warm_cpu", |b| {
            b.iter(|| {
                let mut encoder =
                    g.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("palantir.text_atlas.cpu_prepare"),
                        });
                {
                    let mut ctx = GpuCtx::new(&g.device, &g.queue, &mut belt, &mut encoder);
                    let payloads = scene.store.payloads.borrow();
                    let interned_text = payloads.interned_text();
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
        // Prime the cycle so the LRU has all rungs resident before the
        // measured loop starts evicting + re-inserting.
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
        // Large stable key set: every run hits its own cache row every
        // frame, so nothing expires and nothing is re-encoded — all the
        // encoded cache does is maintenance. CPU-only, because a
        // whole-map scan is microseconds against a GPU submit and the
        // full-frame variant cannot resolve it.
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
                    let payloads = scene.store.payloads.borrow();
                    let interned_text = payloads.interned_text();
                    backend.prepare(&mut ctx, BASE_SCALE, &scene.runs, &interned_text);
                }
                belt.finish();
                belt.recall();
                backend.end_frame();
            });
        });
    }

    {
        // The counter-workload to `stable_keys_cpu`: every frame lands on
        // a new quantized scale, so every run misses, appends to the
        // encoded arena and inserts a row. CPU-only for the same reason
        // — `cache_churn` measures the same scene end to end, but at
        // ~1 ms per GPU-bound iteration it cannot resolve which side of
        // the encoded cache's maintenance tradeoff moved.
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
                    let payloads = scene.store.payloads.borrow();
                    let interned_text = payloads.interned_text();
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

    bench_encoded_cache(c);
}

/// The encoded cache's per-frame maintenance, priced on its own in the
/// two steady states a real frame is ever in. Both run CPU-only: the
/// `text_atlas` arms above can't resolve either (a few percent of those
/// workloads, under this machine's run-to-run drift).
///
/// - **`steady`** — a static text-heavy scene. Nothing expires, so every
///   fired ticket finds its row live and re-files. This is the drain
///   path a still frame pays, and it runs on every frame by design: a
///   cadence gate would trade uniform cost for a periodic spike, so this
///   is the number that has to stay small. 12 glyphs per row matches
///   what the 512-row stable scene actually leaves in the arena (~11.8).
/// - **`churn`** — a zoom or width drag. Every run is re-keyed, so every
///   frame settles a full complement of rows and expires the one from a
///   window ago. This is the only arm that executes `settle`'s
///   allocate-and-copy at all — the per-row cost the block allocator
///   introduced when it replaced an arena append with a copy out of
///   `pending`.
///
/// The two are not redundant: they drive opposite branches of the drain
/// closure, `refiles` against `expiries`, and only one of them allocates.
///
/// **Neither guards uniformity, and neither can.** The compaction the
/// block allocator replaced was amortised free — one frame in 122 paying
/// 122x — so a mean or a median showed nothing and the whole defect
/// lived in the tail. What guards the shape is
/// `a_saturated_gesture_reaches_a_steady_state_where_no_frame_allocates`,
/// which asserts zero allocations and a constant arena outright. These
/// arms guard the *constant*.
fn bench_encoded_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("encoded_cache");
    group.measurement_time(Duration::from_secs(2));

    for rows in [128u32, 512] {
        let mut fixture = SweepBench::new(rows, 12);
        assert_eq!(fixture.sweep_steady(), rows as usize);
        group.bench_with_input(BenchmarkId::new("steady", rows), &rows, |b, _| {
            b.iter(|| black_box(fixture.sweep_steady()));
        });
    }

    // Two sizes, not three: per-glyph cost is flat once the fixed
    // per-frame overhead stops dominating (measured 413 Melem/s at
    // 50x25 against 431 at 200x40), so a middle arm prices nothing the
    // ends don't. The small one is overhead-dominated; the large one is
    // the 6.8 MB-arena shape the block-allocator measurements were taken
    // at, and the only one where cache pressure could show.
    //
    // Warmed past the retention window first, because the interesting
    // state is the saturated one — before that the arena is still
    // growing and every row is a fresh block rather than a recycled one,
    // which is the opposite of what a gesture pays in steady state.
    // Warmed against `RENDERED_RUN_KEEP_FRAMES` rather than the encoded
    // window itself: that constant is the documented *ceiling* on this
    // one, so twice it saturates the population whatever the encoded
    // window is later tuned to, and this arm needs no edit to follow it.
    for (runs, glyphs) in [(8u32, 12u32), (200, 40)] {
        let mut fixture = ChurnBench::new(runs, glyphs);
        for _ in 0..RENDERED_RUN_KEEP_FRAMES * 2 {
            fixture.churn_frame();
        }
        let saturated = fixture.arena_len();
        group.throughput(Throughput::Elements((runs * glyphs) as u64));
        group.bench_with_input(
            BenchmarkId::new("churn", format!("{runs}x{glyphs}")),
            &runs,
            |b, _| b.iter(|| black_box(fixture.churn_frame())),
        );
        // The property the number is priced against: a saturated gesture
        // recycles, so the measured frames must not have grown the
        // arena. A regression here invalidates the number rather than
        // merely changing it.
        assert_eq!(
            fixture.arena_len(),
            saturated,
            "{runs}x{glyphs}: the measured frames grew the arena",
        );
    }

    group.finish();
}
