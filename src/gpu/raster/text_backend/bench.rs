//! Text-backend microbench: `prepare`, `flush` and `render_batch` driven directly against `TextBackend`, bypassing `WindowDriver`.
//!
//! - `steady_warm`: fixed scale, atlas primed; every glyph is an `atlas.touch` hit.
//! - `zoom_smooth`: five resident scale rungs, stepped in turn; every run hits the encoded cache.
//! - `cache_churn`: [`CHURN_SCALE_CYCLE`] rungs in permuted order, past the retention window, so each revisit misses.
//! - `mixed_stable_churn`: half the runs stay at one hot scale while the rest cycle the cold rungs, to see whether atlas pressure rebuilds unrelated stable runs.
//! - `*_cpu`: `prepare` alone, for the CPU cost a GPU submit would drown.

#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::bench::Run;
use crate::common::counters::CounterSet;
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
use std::mem;
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

/// Grey of a run's colour: a label is brighter than a value.
const LABEL: f32 = 0.96;
const VALUE: f32 = 0.86;

/// The raster scale of the `step`th rung `stride` apart.
fn rung(step: u32, stride: f32) -> f32 {
    BASE_SCALE + step as f32 * stride
}

/// The `i`th rung of the churn arms' permuted walk.
fn churn_rung(i: u32) -> f32 {
    rung(
        i.wrapping_mul(CHURN_INDEX_STRIDE) % CHURN_SCALE_CYCLE,
        TEXT_SCALE_STEP,
    )
}

/// One single-line run of `text` at `origin`, shaped into `shaper` under the key it stamps.
fn make_run(
    store: &mut RecordStore,
    shaper: &TextShaper,
    text: &str,
    font_size: f32,
    origin: Vec2,
    grey: f32,
) -> TextDrawRow {
    let interned = store.intern(text);
    let recorded = store.record_text(interned);
    // Warm through the run so the stamped key is the one the shaped buffer landed under (no width, non-binding policy: the unbounded root).
    let run = TextRun {
        text,
        font: GlyphFont {
            line_height: font_size * 1.2,
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
        bounds: URect::new(0, 0, PHYSICAL.x, PHYSICAL.y),
        color: RgbaF16::new(grey, grey, grey, 1.0),
        scale: 1.0,
    }
}

/// A node-editor row repeated [`ROWS`] times: a label and three values.
fn node_rows(store: &mut RecordStore, shaper: &TextShaper) -> Vec<TextDrawRow> {
    const COLUMNS: [(&str, f32, f32, f32); 4] = [
        ("node", 13.0, 16.0, LABEL),
        ("input: f32", 11.0, 80.0, VALUE),
        ("output: Vec3", 11.0, 220.0, VALUE),
        ("123.45", 11.0, 380.0, VALUE),
    ];
    let mut runs = Vec::with_capacity(ROWS as usize * COLUMNS.len());
    for row in 0..ROWS {
        let y = 16.0 + row as f32 * 18.0;
        for (text, size, x, grey) in COLUMNS {
            runs.push(make_run(store, shaper, text, size, Vec2::new(x, y), grey));
        }
    }
    runs
}

/// [`DISTINCT_RUNS`] runs with distinct texts, each in its own encoded-cache row, at integral origins with y inside the viewport (a y-culled run isn't cached).
fn distinct_runs(store: &mut RecordStore, shaper: &TextShaper) -> Vec<TextDrawRow> {
    (0..DISTINCT_RUNS)
        .map(|i| {
            let (row, column) = (i as u32 % ROWS, i as u32 / ROWS);
            let origin = Vec2::new(16.0 + column as f32 * 80.0, 16.0 + row as f32 * 18.0);
            make_run(
                store,
                shaper,
                &format!("field {i}: f32"),
                11.0,
                origin,
                VALUE,
            )
        })
        .collect()
}

/// Runs drawn at one raster scale, as one text batch.
#[derive(Clone, Copy, Debug)]
struct Batch<'a> {
    runs: &'a [TextDrawRow],
    scale: f32,
}

/// A text backend over its own shaper, the runs it draws and the target it draws into.
#[derive(Debug)]
struct Fixture {
    backend: TextBackend,
    pipelines: StencilVariant,
    belt: StagingBelt,
    store: RecordStore,
    runs: Vec<TextDrawRow>,
    target: wgpu::TextureView,
}

impl Fixture {
    fn new(make_runs: fn(&mut RecordStore, &TextShaper) -> Vec<TextDrawRow>) -> Self {
        let g = gpu();
        let shaper = TextShaper::new();
        let mut store = RecordStore::default();
        let runs = make_runs(&mut store, &shaper);
        let raster = RasterProgram::new(&g.gpu.device);
        Self {
            backend: TextBackend::new(&g.gpu.device, &raster, shaper),
            pipelines: raster.build_variants(&g.gpu.device, TARGET_FORMAT),
            belt: StagingBelt::new(g.gpu.device.clone(), 1 << 20),
            store,
            runs,
            target: g.target("palantir.text_atlas.target", PHYSICAL).view(),
        }
    }

    /// One frame of every run at `scale`, submitted and drained.
    fn frame(&mut self, scale: f32) {
        let runs = mem::take(&mut self.runs);
        self.frame_batches(slice::from_ref(&Batch { runs: &runs, scale }));
        self.runs = runs;
    }

    /// One frame drawing `batches` in order, submitted and drained.
    fn frame_batches(&mut self, batches: &[Batch<'_>]) {
        let g = gpu();
        let mut encoder = g
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.text_atlas.encoder"),
            });
        {
            let mut ctx = GpuCtx::new(&g.gpu.device, &g.gpu.queue, &mut self.belt, &mut encoder);
            let interned_text = self.store.interned_text();
            for (index, batch) in batches.iter().enumerate() {
                self.backend.prepare_batch(
                    &mut ctx,
                    batch.scale,
                    index,
                    batch.runs,
                    &interned_text,
                );
            }
            self.backend.flush(&mut ctx);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.text_atlas.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target,
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
            // `render_batch` binds neither pipeline nor viewport (the render loop owns both, see `Bound::Raster`), so a standalone pass binds its own.
            pass.set_pipeline(self.pipelines.select(false));
            ViewportPush { size: Vec2::ZERO }.push_into(&mut pass);
            for index in 0..batches.len() {
                self.backend.render_batch(index, &mut pass);
            }
        }
        self.belt.finish();
        g.gpu.queue.submit([encoder.finish()]);
        self.belt.recall();
        g.wait();
        self.backend.tick_frame();
    }

    /// `prepare` alone at `scale`: no flush, draw or submit. A throwaway encoder and the belt satisfy its signature.
    fn prepare(&mut self, scale: f32) {
        let g = gpu();
        let mut encoder = g
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.text_atlas.prepare"),
            });
        {
            let mut ctx = GpuCtx::new(&g.gpu.device, &g.gpu.queue, &mut self.belt, &mut encoder);
            let interned_text = self.store.interned_text();
            self.backend
                .prepare_batch(&mut ctx, scale, 0, &self.runs, &interned_text);
        }
        self.belt.finish();
        self.belt.recall();
        self.backend.tick_frame();
    }

    /// Reports what the glyph atlas paid to stay packed over `frames` primed frames. Printed before the measured section so it doesn't depend on criterion's iteration count or `--list`.
    fn report_atlas_pressure(&self, label: &str, frames: u32) {
        let atlas = &self.backend.pass.atlas;
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
}

fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::Bare)
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.group(c);

    let mut steady = Fixture::new(node_rows);
    for _ in 0..2 {
        steady.frame(BASE_SCALE);
    }
    group.bench_function("steady_warm", |b| b.iter(|| steady.frame(BASE_SCALE)));
    group.bench_function("steady_warm_cpu", |b| b.iter(|| steady.prepare(BASE_SCALE)));
    drop(steady);

    let mut zoom = Fixture::new(node_rows);
    for step in 0..WARM_SCALE_CYCLE {
        zoom.frame(rung(step, TEXT_SCALE_STEP));
    }
    let mut i = 0u32;
    group.bench_function("zoom_smooth", |b| {
        b.iter(|| {
            zoom.frame(rung(i % WARM_SCALE_CYCLE, TEXT_SCALE_STEP));
            i = i.wrapping_add(1);
        });
    });
    drop(zoom);

    let mut churn = Fixture::new(node_rows);
    for step in 0..CHURN_SCALE_CYCLE {
        churn.frame(rung(step, TEXT_SCALE_STEP));
    }
    churn.report_atlas_pressure("cache_churn", CHURN_SCALE_CYCLE);
    let mut i = 0u32;
    group.bench_function("cache_churn", |b| {
        b.iter(|| {
            churn.frame(churn_rung(i));
            i = i.wrapping_add(1);
        });
    });
    // Counter-workload to `stable_keys_cpu`: every frame lands on a new scale, so every run misses and inserts a row. CPU-only because `cache_churn` (~1 ms, GPU-bound) can't resolve the maintenance tradeoff.
    group.bench_function("churn_cpu", |b| {
        b.iter(|| {
            churn.prepare(churn_rung(i));
            i = i.wrapping_add(1);
        });
    });
    drop(churn);

    let mut mixed = Fixture::new(node_rows);
    let runs = mem::take(&mut mixed.runs);
    let (stable, churning) = runs.split_at(runs.len() / 2);
    let batches = |scale| {
        [
            Batch {
                runs: stable,
                scale: BASE_SCALE,
            },
            Batch {
                runs: churning,
                scale,
            },
        ]
    };
    for step in 0..CHURN_SCALE_CYCLE {
        mixed.frame_batches(&batches(rung(step, TEXT_SCALE_STEP)));
    }
    let mut i = 0u32;
    group.bench_function("mixed_stable_churn", |b| {
        b.iter(|| {
            mixed.frame_batches(&batches(churn_rung(i)));
            i = i.wrapping_add(1);
        });
    });
    drop(mixed);

    // Large stable key set: every run hits its own row, nothing expires or re-encodes. CPU-only, since a whole-map scan is microseconds against a GPU submit.
    let mut distinct = Fixture::new(distinct_runs);
    for _ in 0..2 {
        distinct.frame(BASE_SCALE);
    }
    group.bench_function("stable_keys_cpu", |b| {
        b.iter(|| distinct.prepare(BASE_SCALE));
    });
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
