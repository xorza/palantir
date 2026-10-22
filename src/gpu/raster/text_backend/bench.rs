//! Text-backend microbench: `prepare`, `flush` and `render_batch` driven directly against `TextBackend`, bypassing `WindowDriver`.
//!
//! - `steady_warm`: fixed scale, atlas primed; every glyph is an `atlas.touch` hit.
//! - `zoom_smooth`: five resident scale rungs, stepped in turn; every run hits the encoded cache.
//! - `cache_churn`: [`CHURN_SCALE_CYCLE`] rungs in permuted order, past the retention window, so each revisit misses.
//! - `mixed_stable_churn`: half the runs stay at one hot scale while the rest cycle the cold rungs, to see whether atlas pressure rebuilds unrelated stable runs.
//! - `stable_keys`: one row per run, every run a hit.
//!
//! Each runs as a whole `frame`, and some as `prepare` alone, for the CPU cost a GPU submit would
//! drown.

use crate::bench::fixture::{Backend, Backends, Kind, Scale};
use crate::common::counters::CounterSet;
use crate::gpu::bench_gpu::{BenchGpu, TARGET_FORMAT, Timing};
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::stencil_variant::{StencilMode, StencilVariant};
use crate::gpu::raster::raster_program::RasterProgram;
use crate::gpu::raster::text_backend::encode::cache::internals::{ChurnBench, SweepBench};
use crate::gpu::raster::text_backend::{TextBackend, TextBatchInput};
use crate::gpu::surface::viewport::ViewportPush;
use crate::primitives::geometry::urect::URect;
use crate::primitives::layout::align::Align;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::shape::record_store::RecordStore;
use crate::text::glyph_font::GlyphFont;
use crate::text::run::TextRun;
use crate::text::shaped_ref::ShapedTextRef;
use crate::text::shaper::TextShaper;
use crate::text::wrap::TextWrap;
use crate::text::{RENDERED_RUN_KEEP_FRAMES, TEXT_SCALE_STEP};
use criterion::{Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use std::mem;
use std::slice;
use wgpu::util::StagingBelt;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const BASE_SCALE: f32 = 2.0;
const WARM_SCALE_CYCLE: u32 = 5;

/// Rungs the churn arms cycle through, enough to put the mask atlas past `RasterAtlasConfig::eager_growth_bytes`, where it recycles rectangles (`RasterAtlas::evict_one`); eviction begins between rung 250 and 500. The only arm coverage of that regime, which `Fixture::assert_evicting` checks.
const CHURN_SCALE_CYCLE: u32 = 512;
const CHURN_IDX_STRIDE: u32 = 37;

const ROWS: u32 = 32;

/// Distinct-text run count for the large-stable-key-set workload: the steady scene's runs collapse onto few cache rows (`EncodedKey` folds in only text key, quantized scale, color and subpixel bins), and maintenance scales with rows.
const DISTINCT_RUNS: usize = 512;

/// Gray of a run's color: a label is brighter than a value.
const LABEL: f32 = 0.96;
const VALUE: f32 = 0.86;

/// The raster scale of the `step`th rung `stride` apart.
fn rung(step: u32, stride: f32) -> f32 {
    BASE_SCALE + step as f32 * stride
}

/// The `i`th rung of the churn arms' permuted walk.
fn churn_rung(i: u32) -> f32 {
    rung(
        i.wrapping_mul(CHURN_IDX_STRIDE) % CHURN_SCALE_CYCLE,
        TEXT_SCALE_STEP,
    )
}

/// One single-line run of `text` at `origin`, in `gray`.
#[derive(Clone, Copy, Debug)]
struct BenchRun<'a> {
    text: &'a str,
    font_size: f32,
    origin: Vec2,
    gray: f32,
}

impl BenchRun<'_> {
    /// The run's draw row, shaped into `shaper` under the key it stamps.
    fn row(self, store: &mut RecordStore, shaper: &TextShaper) -> TextDrawRow {
        let Self {
            text,
            font_size,
            origin,
            gray,
        } = self;
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
            color: RgbaF16::new(gray, gray, gray, 1.0),
            scale: 1.0,
        }
    }
}

/// A node-editor row repeated [`ROWS`] times: a label and three values.
fn node_rows(store: &mut RecordStore, shaper: &TextShaper) -> Vec<TextDrawRow> {
    const COLS: [(&str, f32, f32, f32); 4] = [
        ("node", 13.0, 16.0, LABEL),
        ("input: f32", 11.0, 80.0, VALUE),
        ("output: Vec3", 11.0, 220.0, VALUE),
        ("123.45", 11.0, 380.0, VALUE),
    ];
    let mut runs = Vec::with_capacity(ROWS as usize * COLS.len());
    for row in 0..ROWS {
        let y = 16.0 + row as f32 * 18.0;
        for (text, size, x, gray) in COLS {
            let run = BenchRun {
                text,
                font_size: size,
                origin: Vec2::new(x, y),
                gray,
            };
            runs.push(run.row(store, shaper));
        }
    }
    runs
}

/// [`DISTINCT_RUNS`] runs with distinct texts, each in its own encoded-cache row, at integral origins with y inside the viewport (a y-culled run isn't cached).
fn distinct_runs(store: &mut RecordStore, shaper: &TextShaper) -> Vec<TextDrawRow> {
    (0..DISTINCT_RUNS)
        .map(|i| {
            let (row, col) = (i as u32 % ROWS, i as u32 / ROWS);
            let origin = Vec2::new(16.0 + col as f32 * 80.0, 16.0 + row as f32 * 18.0);
            BenchRun {
                text: &format!("field {i}: f32"),
                font_size: 11.0,
                origin,
                gray: VALUE,
            }
            .row(store, shaper)
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
            let mut ctx = GpuCtx::new(&g.gpu, &mut self.belt, &mut encoder);
            let interned_text = self.store.interned_text();
            for (idx, batch) in batches.iter().enumerate() {
                self.backend.prepare_batch(TextBatchInput {
                    idx,
                    runs: batch.runs,
                    scale: batch.scale,
                    interned_text,
                });
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
            pass.set_pipeline(self.pipelines.select(StencilMode::Off));
            ViewportPush { size: Vec2::ZERO }.push_into(&mut pass);
            for idx in 0..batches.len() {
                self.backend.render_batch(idx, &mut pass);
            }
        }
        self.belt.finish();
        g.gpu.queue.submit([encoder.finish()]);
        self.belt.recall();
        g.wait();
        self.backend.tick_frame();
    }

    /// `prepare` alone at `scale`: no flush, draw or submit.
    fn prepare(&mut self, scale: f32) {
        self.backend.prepare_batch(TextBatchInput {
            idx: 0,
            runs: &self.runs,
            scale,
            interned_text: self.store.interned_text(),
        });
        self.backend.tick_frame();
    }

    /// Checks the atlas this fixture primed is recycling rectangles, the regime the churn arms
    /// exist for, and refused no glyph: an entry past the ceiling is refused every frame, so a
    /// fixture producing one measures a permanently degraded atlas.
    fn assert_evicting(&self, label: &str) {
        let counts = self.backend.pass.atlas.counters.counts();
        assert_eq!(
            counts.oversized, 0,
            "{label}: a glyph exceeded the atlas ceiling"
        );
        assert!(
            counts.evictions > 0,
            "{label}: the primed atlas never evicted"
        );
    }
}

/// What a case runs of a frame at a scale: the whole frame, or `prepare` alone.
type Stage = fn(&mut Fixture, f32);

fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::Bare)
}

/// `atomic/text_atlas/gpu/<workload>/<frame|prepare>`: the text backend driven directly, a
/// whole frame (prepare, flush, draw and submit) or `prepare` alone for the CPU cost a submit
/// would drown. Both need the device.
pub(crate) fn atomic(c: &mut Criterion, backends: Backends) {
    bench_encoded_cache(c);
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "text_atlas", Scale::Frame);
    let case = |name: &str| backend.case(name);

    group.bench_function(case("steady_warm/frame"), |b| {
        let mut steady = Fixture::new(node_rows);
        for _ in 0..2 {
            steady.frame(BASE_SCALE);
        }
        b.iter(|| steady.frame(BASE_SCALE));
    });
    group.bench_function(case("steady_warm/prepare"), |b| {
        let mut steady = Fixture::new(node_rows);
        for _ in 0..2 {
            steady.frame(BASE_SCALE);
        }
        b.iter(|| steady.prepare(BASE_SCALE));
    });

    group.bench_function(case("zoom_smooth/frame"), |b| {
        let mut zoom = Fixture::new(node_rows);
        for step in 0..WARM_SCALE_CYCLE {
            zoom.frame(rung(step, TEXT_SCALE_STEP));
        }
        let mut i = 0u32;
        b.iter(|| {
            zoom.frame(rung(i % WARM_SCALE_CYCLE, TEXT_SCALE_STEP));
            i = i.wrapping_add(1);
        });
    });

    // `cache_churn/prepare` is the counter-workload to `stable_keys/prepare`: every frame lands
    // on a new scale, so every run misses and inserts a row.
    let stages: [(&str, Stage); 2] = [("frame", Fixture::frame), ("prepare", Fixture::prepare)];
    for (stage, run_stage) in stages {
        group.bench_function(case(&format!("cache_churn/{stage}")), |b| {
            let mut churn = Fixture::new(node_rows);
            for step in 0..CHURN_SCALE_CYCLE {
                churn.frame(rung(step, TEXT_SCALE_STEP));
            }
            churn.assert_evicting("cache_churn");
            let mut i = 0u32;
            b.iter(|| {
                run_stage(&mut churn, churn_rung(i));
                i = i.wrapping_add(1);
            });
        });
    }

    group.bench_function(case("mixed_stable_churn/frame"), |b| {
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
        b.iter(|| {
            mixed.frame_batches(&batches(churn_rung(i)));
            i = i.wrapping_add(1);
        });
    });

    // Large stable key set: every run hits its own row, nothing expires or re-encodes. `prepare`
    // alone, since a whole-map scan is microseconds against a GPU submit.
    group.bench_function(case("stable_keys/prepare"), |b| {
        let mut distinct = Fixture::new(distinct_runs);
        for _ in 0..2 {
            distinct.frame(BASE_SCALE);
        }
        b.iter(|| distinct.prepare(BASE_SCALE));
    });
    group.finish();
}

/// The encoded cache's per-frame maintenance in two steady states, CPU-only (the `text_atlas` arms can't resolve it under drift).
///
/// - **`steady`**: nothing expires, so every fired ticket re-files; this drain path runs every frame by design (a cadence gate would trade uniform cost for a spike), so it must stay small. 12 glyphs per row matches the 512-row scene (~11.8).
/// - **`churn`**: a zoom or width drag re-keys every run; the only arm running `settle`'s allocate-and-copy.
///
/// **Neither guards uniformity:** the replaced compaction was amortized free (one frame in 122 paying 122x), so the defect lived in the tail. `a_saturated_gesture_reaches_a_steady_state_where_no_frame_allocates` guards the shape; these guard the constant.
///
/// `atomic/encoded_cache/cpu/steady/<rows>` and `atomic/encoded_cache/cpu/churn/<runs>x<glyphs>`.
fn bench_encoded_cache(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "encoded_cache", Scale::Sample);

    for rows in [128u32, 512] {
        group.bench_function(Backend::Cpu.case(&format!("steady/{rows}")), |b| {
            let mut fixture = SweepBench::new(rows, 12);
            assert_eq!(fixture.sweep_steady(), rows as usize);
            b.iter(|| black_box(fixture.sweep_steady()));
        });
    }

    // Two sizes: per-glyph cost is flat once fixed overhead stops dominating (413 Melem/s at 50x25 against 431 at 200x40); the large one is the 6.8 MB-arena shape where cache pressure could show.
    //
    // Warmed past the retention window (against `RENDERED_RUN_KEEP_FRAMES`, the documented ceiling), since the arena is still growing before saturation.
    for (runs, glyphs) in [(8u32, 12u32), (200, 40)] {
        group.throughput(Throughput::Elements(u64::from(runs * glyphs)));
        group.bench_function(Backend::Cpu.case(&format!("churn/{runs}x{glyphs}")), |b| {
            let mut fixture = ChurnBench::new(runs, glyphs);
            for _ in 0..RENDERED_RUN_KEEP_FRAMES * 2 {
                fixture.churn_frame();
            }
            let saturated = fixture.arena_len();
            b.iter(|| black_box(fixture.churn_frame()));
            // A saturated gesture recycles, so the arena must not have grown; a regression
            // invalidates the number.
            assert_eq!(
                fixture.arena_len(),
                saturated,
                "{runs}x{glyphs}: the measured frames grew the arena",
            );
        });
    }

    group.finish();
}
