//! Command-recording benchmark: the host CPU cost of translating one frame's `RenderStep` stream into wgpu commands inside `WgpuBackend::run_main_pass`.
//!
//! This cost scales with draw-step count, not pixels, and no other benchmark sees it.
//!
//! Every arm is a pair painting the same content, one paying per item and one paying once; the difference is the headroom for a change that collapses bind / draw / state-set count.
//!
//! - `groups/per_item` vs `groups/single`: N clipped cells (a `DrawGroup` each) against N unclipped rects folded into one group and draw.
//! - `images/distinct` vs `images/shared`: N textures against one. `distinct` is the control that must not regress.
//! - `text/per_group` vs `text/single`: N clipped text cells (N batches) against N runs in one batch. Net out the per-step rate from the `groups` pair first: splitting batches also churns scissors.
//!
//! Method: GPU instrumentation stays off so no timestamp writes land in the measured pass. Each arm renders `WARMUP_FRAMES`, samples `last_main_pass_cpu` over `EVIDENCE_FRAMES` and reports min and median; min is the keep-or-revert signal. Criterion measures the same window via `iter_custom` but its interval stays wide: use it for ordering and large regressions.
//!
//! Whole-frame wall time is never reported: it moves the opposite way across the `groups` pair.

use crate::bench::fixture::{Backends, Kind, Scale};
use crate::gpu::bench_gpu::{BenchGpu, BenchTarget, Timing};
use crate::host::offscreen::OffscreenHost;
use crate::image_registry::image_handle::ImageHandle;
use crate::internals::record_app::RecordApp;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{Image, ImageFit};
use crate::shape::Shape;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use criterion::{Criterion, Throughput};
use glam::UVec2;
use glam::Vec2;
use std::hint::black_box;
use std::time::Duration;

const PHYSICAL: UVec2 = UVec2::new(512, 512);
/// Cells per axis. `GRID * GRID` items tile the viewport with no gaps, so dirty rects merge past `FULL_REPAINT_THRESHOLD` and every frame stays a single `Full` walk; `Fixture::frame` asserts it.
const GRID: usize = 16;
const ITEMS: usize = GRID * GRID;
const CELL: f32 = PHYSICAL.x as f32 / GRID as f32;
const TEXEL: u32 = 8;
/// Text-arm run content. Must shape wider than `CELL` so a clipped cell cuts it in X, which marks the run strict and splits the text batch. The default `SingleLine` overflows rather than wraps.
const LABEL: &str = "Palantir record pass";
/// Frames rendered before sampling, enough to settle the glyph atlas, image bind-group cache and buffer growth.
const WARMUP_FRAMES: usize = 64;

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

    /// Whether each cell clips. Only the per-item arms do: a clip gives each its own scissor (a `DrawGroup`) and cuts `LABEL` in X (strict run, split batch). The image pair is unclipped so it differs only in `TextureId` repeats.
    const fn clipped(self) -> bool {
        matches!(self, Self::GroupPerItem | Self::TextPerGroup)
    }

    /// The draw groups and text batches the arm's frame composes: a clipped cell is a group and
    /// splits its text batch, an unclipped frame folds into one group, and no text means no
    /// batch.
    const fn expected(self) -> Counts {
        let (groups, text_batches) = match self {
            Self::GroupPerItem => (ITEMS, 0),
            Self::TextPerGroup => (ITEMS, ITEMS),
            Self::SingleGroup | Self::ImagesDistinct | Self::ImagesShared => (1, 0),
            Self::TextSingleBatch => (1, 1),
        };
        Counts {
            groups,
            text_batches,
        }
    }

    /// Textures the image arms register: `ImagesDistinct` one per item, `ImagesShared` one for all. Zero for the other arms.
    const fn textures(self) -> usize {
        match self {
            Self::ImagesDistinct => ITEMS,
            Self::ImagesShared => 1,
            _ => 0,
        }
    }
}

/// Device without `TIMESTAMP_QUERY`, so a future default cannot write timestamps into the timed pass.
fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::Bare)
}

fn texels(seed: usize) -> Vec<u8> {
    let tone = (seed % 251) as u8;
    let mut pixels = Vec::with_capacity((TEXEL * TEXEL * 4) as usize);
    for _ in 0..TEXEL * TEXEL {
        pixels.extend_from_slice(&[tone, 255 - tone, 128, 255]);
    }
    pixels
}

fn cell_origin(i: usize) -> Vec2 {
    Vec2::new((i % GRID) as f32 * CELL, (i / GRID) as f32 * CELL)
}

/// Per-frame paint toggle: every color flips so the frame stays `Full`, while geometry stays cached.
const fn tint(phase: bool) -> RgbaF32 {
    if phase {
        RgbaF32::WHITE
    } else {
        RgbaF32::srgb(0.85, 0.9, 1.0)
    }
}

/// One frame's scene: `workload`'s items in `color`, drawing `handles` where it shows images.
#[derive(Debug)]
struct Scene<'a> {
    handles: &'a [ImageHandle],
    workload: Workload,
    color: RgbaF32,
}

impl Scene<'_> {
    fn record(&self, ui: &mut Ui) {
        let Self {
            handles,
            workload,
            color,
        } = *self;
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
}

/// The draw-list shape behind one arm's timing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    groups: usize,
    text_batches: usize,
}

#[derive(Debug)]
struct Fixture {
    host: OffscreenHost,
    target: BenchTarget,
    handles: Vec<ImageHandle>,
    workload: Workload,
    phase: bool,
}

impl Fixture {
    fn new(gpu: &BenchGpu, workload: Workload) -> Self {
        let mut host = gpu.plain_host(false);
        let handles = (0..workload.textures())
            .map(|seed| {
                host.ui()
                    .load_image(
                        &Image::from_srgba8(UVec2::new(TEXEL, TEXEL), texels(seed)).unwrap(),
                    )
                    .expect("benchmark image fits every supported GPU")
            })
            .collect();
        Self {
            host,
            target: gpu.target("palantir.record_pass_bench.target", PHYSICAL),
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
        let scene = Scene {
            handles,
            workload: *workload,
            color: tint(*phase),
        };
        let mut app = RecordApp::new(|ui| scene.record(ui));
        let report = host.frame(target.as_target(), 1.0, &mut app);
        // A `Partial` frame walks the schedule once per damage rect, which would make the numbers incomparable.
        assert_eq!(
            report.paint(),
            FramePaint::Full,
            "{} must repaint fully every frame",
            workload.label()
        );
        // Drain so the staging belt recalls chunks (its map callback fires only on a poll). `Wait` not `Poll`: frames in flight bias recording.
        gpu().wait();
    }

    /// The composed frame's draw-list shape.
    const fn counts(&self) -> Counts {
        let buffer = self.host.last_render_buffer();
        Counts {
            groups: buffer.groups.len(),
            text_batches: buffer.text_batches.len(),
        }
    }

    fn record_time(&self) -> Duration {
        self.host
            .gpu_pass_stats()
            .last_main_pass_cpu()
            .expect("run_main_pass publishes its CPU time on every submitted frame")
    }
}

/// `atomic/record_pass/gpu/<workload>`: one frame's `RenderStep` stream translated into wgpu
/// commands, timed by the pass's own CPU clock, not the frame's wall time. The draw-list shape
/// is checked first. Per item.
pub(crate) fn atomic(c: &mut Criterion, backends: Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "record_pass", Scale::Sample);
    group.throughput(Throughput::Elements(ITEMS as u64));
    for workload in Workload::ALL {
        group.bench_function(backend.case(workload.label()), |bencher| {
            let mut fixture = Fixture::new(gpu(), workload);
            for _ in 0..WARMUP_FRAMES {
                fixture.frame();
            }
            assert_eq!(
                fixture.counts(),
                workload.expected(),
                "{}",
                workload.label()
            );
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
