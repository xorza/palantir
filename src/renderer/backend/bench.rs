//! Command-recording benchmark: the host CPU cost of translating one
//! frame's `RenderStep` stream into wgpu commands, i.e. everything
//! `WgpuBackend::run_main_pass` does between opening the main render pass
//! and the end-of-pass command replay its drop runs.
//!
//! This is the one frame cost that scales with the *number* of draw steps
//! rather than the number of pixels, and no other benchmark sees it. The
//! `schedule` bench measures `for_each_step` alone — the pure step
//! emitter, no wgpu. The `image_pipeline` / `curve_pipeline` GPU benches
//! are deliberately fragment-bound. `frame/*_gpu` covers recording only
//! as a sliver of a whole-frame number dominated by GPU execution.
//!
//! Every arm is a **pair**: one shape that pays per item and one that
//! pays once, with the *same* painted content in both. What moves between
//! the two is bind / draw / state-set count, so the difference is the
//! headroom available to a change that collapses those.
//!
//! - `groups/per_item` vs `groups/single` — N clipped cells (each its own
//!   scissor, so each opens a `DrawGroup`) against the same N rects
//!   unclipped, which the composer folds into one group and one
//!   instanced draw. Bounds what per-step overhead costs at all, and is
//!   the fixture for bind-tracking work.
//! - `images/distinct` vs `images/shared` — N images on N textures
//!   against N images on one. Both record N binds + N single-instance
//!   draws today; run-coalescing would collapse `shared` to one of each
//!   and must leave `distinct` alone. `distinct` is therefore the control
//!   that must not regress, not a second workload.
//! - `text/per_group` vs `text/single` — N clipped text cells (N text
//!   batches, five unconditional commands each) against N runs in one
//!   batch. `TextBackend::render_batch` opts out of the backend's bind
//!   tracking entirely, so the gap here is what letting it participate
//!   could recover.
//!
//! Read the text pair with one correction. Nothing splits a text batch
//! on a plain scissor change, so the only way to *get* consecutive text
//! batches is a split that also churns scissors — `per_group` records
//! two scissor steps per item on top of its batch. Its gap over `single`
//! therefore bundles generic per-step cost with the per-batch text cost.
//! Net the first out with the per-step rate the `groups` pair measures
//! (its gap divided by its own step gap) before crediting anything to
//! text. All three arms print their step and scissor counts for exactly
//! this arithmetic.
//!
//! Method: GPU instrumentation stays **off**, so `gpu_timings` is `None`
//! and no timestamp writes land inside the pass being measured — the
//! numbers would otherwise include the commands the measurement added.
//! Each arm renders `WARMUP_FRAMES` frames, then samples
//! `last_main_pass_cpu_ms` over `EVIDENCE_FRAMES` and reports min /
//! median. Min is the keep-or-revert signal (the upper half measures
//! interference from the rest of the machine, not recording). Criterion
//! measures the same window through `iter_custom`, which keeps saved
//! baselines and the per-step throughput rate working — but harvesting
//! one ~10 µs sample costs a whole frame, so its budget buys few
//! iterations per sample and its interval stays wide (tens of percent).
//! Read criterion for the ordering and for catching a large regression;
//! read the min for anything finer.
//!
//! Whole-frame wall time is deliberately never reported. A frame is
//! ~70x the recording it contains, and its frontend costs move the
//! *opposite* way across the `groups` pair, so it ranks the arms
//! backwards.
//!
//! Step and draw-list counts print alongside each result. They explain a
//! result — they don't replace its elapsed time.

use crate::app::internals::RecordApp;
use crate::bench::Run;
use crate::host::bench_gpu::{BenchGpu, Timing};
use crate::host::offscreen::{OffscreenHost, test_support as offscreen_support};
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::primitives::image::{Image, ImageFit};
use crate::primitives::rect::Rect;
use crate::renderer::backend::schedule::test_support::Walk;
use crate::renderer::image_registry::ImageHandle;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::scene::node::configure::Configure;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;
use criterion::{Criterion, Throughput};
use glam::Vec2;
use std::hint::black_box;
use std::time::Duration;

const PHYSICAL: glam::UVec2 = glam::UVec2::new(512, 512);
/// Cells per axis. `GRID * GRID` items tile the viewport exactly, with no
/// gaps — every arm's dirty rects then merge into a region covering well
/// over `FULL_REPAINT_THRESHOLD`, which is what keeps each frame a single
/// `Full` walk. A gapped or centered layout could settle into `Partial`
/// and silently start measuring up to `DAMAGE_RECT_CAP` walks instead;
/// `Fixture::frame` asserts against that.
const GRID: usize = 16;
const ITEMS: usize = GRID * GRID;
const CELL: f32 = PHYSICAL.x as f32 / GRID as f32;
/// Edge of each source texture in the image arms. Tiny on purpose: these
/// arms measure binds and draws, and a large texture would only add
/// upload and sampling cost outside the measured window.
const TEXEL: u32 = 8;
/// Text-arm run content. Must shape wider than `CELL` at the default
/// style so a clipped cell cuts it in X — that cut is what marks the run
/// *strict* and splits the text batch. `TextWrap`'s default `SingleLine`
/// overflows a narrow slot rather than wrapping, so the width survives
/// the `Fixed(CELL)` cell intact.
const LABEL: &str = "Palantir record pass";
/// Frames rendered before sampling starts — enough to settle the glyph
/// atlas, the image bind-group cache, and every dynamic buffer's capacity
/// growth, so no sample includes a first-touch allocation.
const WARMUP_FRAMES: usize = 64;
const EVIDENCE_FRAMES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Workload {
    GroupPerItem,
    SingleGroup,
    ImagesDistinct,
    ImagesShared,
    TextPerGroup,
    TextSingleBatch,
}

impl Workload {
    const ALL: [Self; 6] = [
        Self::GroupPerItem,
        Self::SingleGroup,
        Self::ImagesDistinct,
        Self::ImagesShared,
        Self::TextPerGroup,
        Self::TextSingleBatch,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::GroupPerItem => "groups/per_item",
            Self::SingleGroup => "groups/single",
            Self::ImagesDistinct => "images/distinct",
            Self::ImagesShared => "images/shared",
            Self::TextPerGroup => "text/per_group",
            Self::TextSingleBatch => "text/single",
        }
    }

    /// Whether each cell clips. Only the two per-item arms do, and for
    /// two different reasons: a clipping cell carries its own scissor,
    /// which opens a `DrawGroup` per item, and it cuts `LABEL`'s extent in
    /// X, which makes each text run *strict* and so splits the text batch
    /// (a plain scissor change does not — text batches deliberately span
    /// groups; see `Composer::close_batch`). The image arms are both
    /// unclipped on purpose: their pair must differ **only** in whether
    /// adjacent draws repeat a `TextureId`, so both sit in one group and
    /// one batch.
    const fn clipped(self) -> bool {
        matches!(self, Self::GroupPerItem | Self::TextPerGroup)
    }

    /// Textures the image arms register. `ImagesDistinct` gives every
    /// item its own so no two adjacent draws share a bind group;
    /// `ImagesShared` gives them all one, the run a coalescing pass would
    /// collapse. Zero for the non-image arms.
    const fn textures(self) -> usize {
        match self {
            Self::ImagesDistinct => ITEMS,
            Self::ImagesShared => 1,
            _ => 0,
        }
    }
}

/// Device without `TIMESTAMP_QUERY`: the host also passes
/// `collect_gpu_stats(false)`, but requesting the feature at all would
/// leave the door open for a future default that writes timestamps into
/// the very pass this benchmark times.
fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::Bare)
}

/// Solid `TEXEL`-square source. Content is irrelevant to a bind/draw
/// count — `seed` only varies it so two registrations can't be folded
/// together by any future content-hash dedup in the image registry.
fn texels(seed: usize) -> Vec<u8> {
    let tone = (seed % 251) as u8;
    let mut pixels = Vec::with_capacity((TEXEL * TEXEL * 4) as usize);
    for _ in 0..TEXEL * TEXEL {
        pixels.extend_from_slice(&[tone, 255 - tone, 128, 255]);
    }
    pixels
}

/// Cell `i`'s top-left corner, walking the grid in row-major order so
/// items are laid out in the same order they were recorded — adjacent
/// draws are adjacent on screen, and no cell overlaps another.
fn cell_origin(i: usize) -> Vec2 {
    Vec2::new((i % GRID) as f32 * CELL, (i / GRID) as f32 * CELL)
}

/// Per-frame paint toggle. Every item's colour flips each frame, so the
/// whole viewport is dirty and the frame stays `Full`. Geometry never
/// changes, which keeps layout and measure fully cached — the record
/// closure is the only thing that re-runs.
fn tint(phase: bool) -> Color {
    if phase {
        Color::WHITE
    } else {
        Color::rgb(0.85, 0.9, 1.0)
    }
}

fn record(ui: &mut Ui, handles: &[ImageHandle], workload: Workload, phase: bool) {
    let color = tint(phase);
    let style = TextStyle::default().with_color(color);
    Panel::canvas()
        .id_salt("record-bench-root")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for i in 0..ITEMS {
                let cell = Panel::zstack()
                    .id_salt(("record-bench-cell", i))
                    .position(cell_origin(i))
                    .size((Sizing::fixed(CELL), Sizing::fixed(CELL)));
                let cell = if workload.clipped() {
                    cell.clip_rect()
                } else {
                    cell
                };
                cell.show(ui, |ui| match workload {
                    Workload::GroupPerItem | Workload::SingleGroup => {
                        ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, CELL, CELL)).fill(color));
                    }
                    Workload::ImagesDistinct | Workload::ImagesShared => {
                        ui.add_shape(
                            Shape::image(handles[i % handles.len()].clone())
                                .fit(ImageFit::Fill)
                                .tint(color),
                        );
                    }
                    Workload::TextPerGroup | Workload::TextSingleBatch => {
                        Text::new(LABEL).style(&style).show(ui);
                    }
                });
            }
        });
}

/// Draw-list shape behind one arm's timing: what the composer produced
/// and what the schedule made of it. `steps` is the exact number of
/// `RenderStep`s the measured pass dispatched.
#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    groups: usize,
    steps: usize,
    scissors: usize,
    quads: usize,
    images: usize,
    image_batches: usize,
    text_batches: usize,
}

#[derive(Debug)]
struct Fixture {
    host: OffscreenHost,
    target: wgpu::Texture,
    handles: Vec<ImageHandle>,
    workload: Workload,
    phase: bool,
}

impl Fixture {
    fn new(gpu: &BenchGpu, workload: Workload) -> Self {
        let mut host = gpu.offscreen_builder().build();
        // No theme panel background: each arm should record exactly the
        // one shape family it names, not that plus a chrome quad per cell.
        host.ui().theme_mut().panel_background = None;
        let handles = (0..workload.textures())
            .map(|seed| {
                host.ui()
                    .register_image(Image::from_rgba8(TEXEL, TEXEL, texels(seed)))
                    .expect("benchmark image fits every supported GPU")
            })
            .collect();
        Self {
            host,
            target: gpu.target(PHYSICAL, "palantir.record_pass_bench.target"),
            handles,
            workload,
            phase: false,
        }
    }

    fn frame(&mut self) {
        self.phase = !self.phase;
        let Self {
            host,
            target,
            handles,
            workload,
            phase,
        } = self;
        let workload = *workload;
        let phase = *phase;
        let mut app = RecordApp::new(|ui| record(ui, handles, workload, phase));
        let report = host.frame_offscreen(target, 1.0, &mut app);
        // A `Partial` frame walks the schedule once per damage rect, so a
        // drift to Partial would quietly turn every number below into a
        // multi-walk measurement that isn't comparable across arms.
        assert_eq!(
            report.paint(),
            FramePaint::Full,
            "{} must repaint fully every frame",
            workload.label()
        );
        // Drain before recording the next frame. Two reasons, and the
        // second is why `Wait` rather than `Poll`:
        //
        // - The staging belt recalls its chunks from a map callback that
        //   only fires on a poll. Unpolled, every frame allocates a fresh
        //   chunk and the arm ends up measuring belt growth.
        // - Recording into a device with frames still in flight is
        //   measurably noisier and, worse, *biased*: under `Poll` the
        //   `images` pair inverts, because queued submissions contend with
        //   the very command recording being timed. `Wait` costs a GPU
        //   round-trip per frame — which lands outside the measured window
        //   — and buys samples that are comparable across arms.
        gpu().wait();
    }

    /// Replay the schedule over the frame just composed to count the
    /// steps the measured pass dispatched. Runs outside the measured
    /// window, on the same `damage = None` full walk `run_main_pass` used.
    fn counts(&self) -> Counts {
        let buffer = offscreen_support::last_render_buffer(&self.host);
        let walk = Walk::new(buffer);
        let counts = walk.run(buffer, None, false);
        Counts {
            groups: buffer.groups.len(),
            steps: counts.steps,
            scissors: counts.scissors,
            quads: buffer.quads.len(),
            images: buffer.images.len(),
            image_batches: buffer.batches(PaintTier::Image).len(),
            text_batches: buffer.text_batches.len(),
        }
    }

    fn record_ms(&self) -> f32 {
        self.host
            .gpu_pass_stats()
            .last_main_pass_cpu_ms()
            .expect("run_main_pass publishes its CPU time on every submitted frame")
    }

    /// The same sample as [`Self::record_ms`], in the form criterion's
    /// `iter_custom` accumulates. f32 milliseconds hold a microsecond-scale
    /// value to ~11 significant digits, so the conversion is lossless at
    /// the magnitudes here.
    fn record_time(&self) -> Duration {
        Duration::from_secs_f32(self.record_ms() / 1_000.0)
    }
}

/// Sorted-sample summary. The minimum is the keep-or-revert signal: the
/// benchmark shares a machine with everything else running on it, so the
/// upper half of the distribution measures interference, not recording.
#[derive(Clone, Copy, Debug)]
struct Summary {
    min: f32,
    median: f32,
}

fn summarize(values: &mut [f32]) -> Summary {
    assert!(!values.is_empty());
    values.sort_unstable_by(f32::total_cmp);
    Summary {
        min: values[0],
        median: values[values.len() / 2],
    }
}

fn report_evidence(fixture: &mut Fixture) -> Counts {
    for _ in 0..WARMUP_FRAMES {
        fixture.frame();
    }
    let counts = fixture.counts();
    let mut samples = Vec::with_capacity(EVIDENCE_FRAMES);
    for _ in 0..EVIDENCE_FRAMES {
        fixture.frame();
        samples.push(fixture.record_ms());
    }
    let summary = summarize(&mut samples);
    eprintln!(
        "[record_pass] {} items={ITEMS} record_min_ms={:.4} record_median_ms={:.4} \
         steps={} groups={} scissors={} quads={} images={} image_batches={} text_batches={}",
        fixture.workload.label(),
        summary.min,
        summary.median,
        counts.steps,
        counts.groups,
        counts.scissors,
        counts.quads,
        counts.images,
        counts.image_batches,
        counts.text_batches,
    );
    counts
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let gpu = gpu();
    eprintln!(
        "[record_pass] adapter={} backend={:?}",
        gpu.info.name, gpu.info.backend,
    );

    let mut group = run.group(c);
    // `iter_custom` reports only the recording window, but criterion still
    // has to pay a whole ~0.5 ms frame to harvest each ~microsecond
    // sample. Left at the defaults it would size its iteration count off
    // the sample and spend minutes of wall clock per arm. These budgets
    // buy thousands of iterations per sample — enough for a tight
    // estimate — in a few seconds.
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(5));
    group.measurement_time(Duration::from_millis(50));
    for workload in Workload::ALL {
        let mut fixture = Fixture::new(gpu, workload);
        let counts = report_evidence(&mut fixture);
        // Per-step, not per-frame: the arms differ by two orders of
        // magnitude in step count, and per-step cost is the comparable
        // quantity across a pair.
        group.throughput(Throughput::Elements(counts.steps as u64));
        group.bench_function(workload.label(), |bencher| {
            // Whole-frame wall time is NOT a usable proxy here and is
            // deliberately not reported: a frame is ~70x the recording it
            // contains, and the frontend costs that dominate it move the
            // *opposite* way across the `groups` pair (one big group prunes
            // occlusion over 256 quads at once; 256 small ones don't). It
            // ranks the arms backwards. Only the recording window counts.
            bencher.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    fixture.frame();
                    total += black_box(fixture.record_time());
                }
                total
            });
        });
    }
    group.finish();
}
