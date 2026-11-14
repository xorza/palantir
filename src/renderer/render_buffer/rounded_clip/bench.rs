//! The rounded clip's benches, over a made scene: 48 cards a 1440p window holds, each a list of
//! rows (a fill, a label, an image and a polyline), so every family that draws under a clip draws
//! under one. Its arms:
//!
//! - `rect`: the cards clip to their rects, the control;
//! - `inside`: rounded clips, the rows inset past every arc;
//! - `edge`: rounded clips, the rows crossing every arc;
//! - `nested`: three rounded clips a card, none holding the next.
//!
//! The cases:
//!
//! - `atomic/clip_pass/gpu/<arm>`: the device time of the main pass, a whole repaint a frame.
//! - `atomic/clip_pass_batch/gpu/<kind>/<arm>`: one batch kind's share of it, from in-pass
//!   timestamps; only where the adapter writes them.
//! - `integration/clip_frame/<backend>/<arm>/<frame>`: whole frames: `scrolling` moves the scene a
//!   step a frame, a whole repaint; `partial` changes a counter, one small rect.

use crate::bench::backends::{Backend, Backends};
use crate::bench::case_group::CaseGroup;
use crate::bench::fixture;
use crate::bench::kind::{Kind, Scale};
use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE};
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::fmt;
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::HarnessDepth;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::image_paint::ImagePaint;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use criterion::Criterion;
use glam::{UVec2, Vec2};

/// Cards a row and rows of cards.
const COLS: usize = 8;
const ROWS: usize = 6;
const CARDS: usize = COLS * ROWS;
/// A card's logical size, and the gap around it.
const CARD: Vec2 = Vec2::new(150.0, 100.0);
const GAP: f32 = 8.0;
/// A card's radius; a nested card's clip steps in by [`NEST_STEP`] and loses as much radius, so
/// its rect grown by a pixel crosses the outer arc and neither clip holds the other.
const RADIUS: f32 = 16.0;
const NEST_STEP: f32 = 2.0;
/// Rows a card lists.
const LIST_ROWS: usize = 4;
const LABELS: [&str; LIST_ROWS] = ["Exposure", "Contrast", "Highlights", "Shadows"];
/// The thumbnail's texels a side.
const THUMB: u32 = 16;
/// Where the counter of the `partial` frames sits, below every card, so its damage holds no clip.
const COUNTER_AT: Vec2 = Vec2::new(8.0, 696.0);
/// Frames each case runs before it checks its paint and starts timing.
const SETTLE_FRAMES: u32 = 4;

/// How the cards clip and where their rows lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipArm {
    Rect,
    Inside,
    Edge,
    Nested,
}

impl ClipArm {
    const ALL: [Self; 4] = [Self::Rect, Self::Inside, Self::Edge, Self::Nested];

    const fn label(self) -> &'static str {
        match self {
            Self::Rect => "rect",
            Self::Inside => "inside",
            Self::Edge => "edge",
            Self::Nested => "nested",
        }
    }

    const fn clip(self) -> ClipMode {
        match self {
            Self::Rect => ClipMode::Rect,
            Self::Inside | Self::Edge | Self::Nested => ClipMode::Rounded,
        }
    }

    /// How far the rows stand inside their card: past the arcs for `inside`, none otherwise.
    const fn inset(self) -> f32 {
        match self {
            Self::Inside => RADIUS,
            Self::Rect | Self::Edge | Self::Nested => 0.0,
        }
    }

    /// Rounded clips a card nests.
    const fn depth(self) -> usize {
        match self {
            Self::Rect => 0,
            Self::Inside | Self::Edge => 1,
            Self::Nested => 3,
        }
    }

    /// The masks the frame's clip pool holds: each card's chains, the one at depth `d` holding
    /// `d` masks, so `1 + … + depth` a card.
    const fn masks(self) -> usize {
        let depth = self.depth();
        CARDS * depth * (depth + 1) / 2
    }
}

/// What a frame changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameArm {
    Scrolling,
    Partial,
}

impl FrameArm {
    const ALL: [Self; 2] = [Self::Scrolling, Self::Partial];

    const fn label(self) -> &'static str {
        match self {
            Self::Scrolling => "scrolling",
            Self::Partial => "partial",
        }
    }

    /// The masks a frame of `arm` composes: a whole repaint all of them, the counter's rect none.
    const fn masks(self, arm: ClipArm) -> usize {
        match self {
            Self::Scrolling => arm.masks(),
            Self::Partial => 0,
        }
    }

    const fn paint(self) -> FramePaint {
        match self {
            Self::Scrolling => FramePaint::Full,
            Self::Partial => FramePaint::Partial,
        }
    }
}

/// One frame of the scene: the cards of `arm`, moved by `scroll`, and a counter at `tick`.
#[derive(Debug)]
struct ClipScene {
    arm: ClipArm,
    thumb: ImageHandle,
    scroll: Vec2,
    tick: u32,
}

impl ClipScene {
    fn new(ui: &Ui, arm: ClipArm) -> Self {
        let texels = (0..THUMB * THUMB)
            .flat_map(|i| {
                let tone = (i * 7 % 256) as u8;
                [tone, 255 - tone, 96, 255]
            })
            .collect();
        let thumb = ui
            .load_image(&Image::from_srgba8(UVec2::splat(THUMB), texels).unwrap())
            .expect("a bench thumbnail fits every supported GPU");
        Self {
            arm,
            thumb,
            scroll: Vec2::ZERO,
            tick: 0,
        }
    }

    /// Move the scene by a step or count a frame, as `frame` says.
    fn advance(&mut self, frame: FrameArm) {
        match frame {
            FrameArm::Scrolling => {
                self.scroll.x = (self.scroll.x + 1.5) % 32.0;
                self.scroll.y = (self.scroll.y + 0.5) % 16.0;
            }
            FrameArm::Partial => self.tick = self.tick.wrapping_add(1),
        }
    }

    fn record(&self, ui: &mut Ui) {
        Panel::zstack()
            .id_salt("clip-bench")
            .size(SizeSpec::FILL)
            .show(ui, |ui| {
                Panel::canvas()
                    .id_salt("clip-bench-cards")
                    .size(SizeSpec::FILL)
                    .transform(TranslateScale::from_translation(self.scroll))
                    .show(ui, |ui| {
                        for card in 0..CARDS {
                            self.nest(ui, card, 0);
                        }
                    });
                Panel::canvas()
                    .id_salt("clip-bench-counter")
                    .size(SizeSpec::FILL)
                    .show(ui, |ui| {
                        Text::new(fmt!(ui, "Frame {:08}", self.tick))
                            .id_salt("clip-bench-counter-text")
                            .at(COUNTER_AT)
                            .size((Sizing::fixed(120.0), Sizing::HUG))
                            .show(ui);
                    });
            });
    }

    /// Card `card`'s clip `level` deep, inside the one above it, and what it holds: the clip
    /// below, or at the arm's depth the rows. Each level steps in by [`NEST_STEP`] and loses as
    /// much radius.
    fn nest(&self, ui: &mut Ui, card: usize, level: usize) {
        let step = level as f32 * NEST_STEP;
        let at = if level == 0 {
            Vec2::new(
                GAP + (card % COLS) as f32 * (CARD.x + GAP),
                GAP + (card / COLS) as f32 * (CARD.y + GAP),
            )
        } else {
            Vec2::splat(NEST_STEP)
        };
        let size = CARD - Vec2::splat(2.0 * step);
        Panel::canvas()
            .id_salt(("clip-bench-card", card, level))
            .at(at)
            .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
            .chrome(Chrome::rounded(
                RgbaF32::TRANSPARENT,
                Corners::all(RADIUS - step),
            ))
            .clip(self.arm.clip())
            .show(ui, |ui| {
                if level + 1 < self.arm.depth() {
                    self.nest(ui, card, level + 1);
                } else {
                    self.rows(ui, size);
                }
            });
    }

    /// The rows of a card's list over a box of `size`, inset by the arm's inset.
    fn rows(&self, ui: &mut Ui, size: Vec2) {
        let inset = self.arm.inset();
        let list = Rect::new(inset, inset, size.x - 2.0 * inset, size.y - 2.0 * inset);
        let row_h = list.size.h / LIST_ROWS as f32;
        for (r, label) in LABELS.into_iter().enumerate() {
            let row = Rect::new(
                list.min.x,
                list.min.y + r as f32 * row_h,
                list.size.w,
                row_h,
            );
            let fill = if r % 2 == 0 {
                RgbaF32::srgb(0.18, 0.2, 0.24)
            } else {
                RgbaF32::srgb(0.24, 0.26, 0.3)
            };
            ui.add_shape(Shape::rect().at(row), fill);
            let thumb = Rect::new(row.max().x - row_h, row.min.y, row_h, row_h);
            ui.add_shape(
                Shape::rect().at(thumb),
                ImagePaint::new(self.thumb.clone()).fit(Fit::Fill),
            );
            let points = [
                Vec2::new(0.0, row_h * 0.8),
                Vec2::new(row.size.w * 0.3, row_h * 0.3),
                Vec2::new(row.size.w * 0.6, row_h * 0.7),
                Vec2::new(row.size.w, row_h * 0.2),
            ];
            ui.add_shape(
                Shape::polyline(&points).at(row),
                Stroke::new(RgbaF32::srgb(0.4, 0.7, 1.0), 1.5),
            );
            Text::new(label)
                .id_salt(("clip-bench-label", r))
                .at(row.min)
                .size((Sizing::fixed(row.size.w - row_h), Sizing::HUG))
                .show(ui);
        }
    }
}

/// A window of the bench's surface and scale at `depth`, whose panels paint no theme chrome; it
/// collects GPU stats where `stats` says.
fn window(depth: HarnessDepth, stats: bool) -> Harness {
    let mut window = Harness::builder(BENCH_SURFACE)
        .system_scale(BENCH_DPR)
        .swapchain_present()
        .collect_gpu_stats(stats)
        .depth(depth)
        .build();
    window.ui_mut().theme_mut().panel_chrome = None;
    window
}

/// A window whose every frame paints whole: its clear alternates, as the backdrop benches' does.
#[derive(Debug)]
struct WholeFrames {
    window: Harness,
    odd: bool,
}

impl WholeFrames {
    fn new(gpu: &BenchGpu) -> Self {
        Self {
            window: window(HarnessDepth::gpu(gpu.handles()), true),
            odd: false,
        }
    }

    fn frame(&mut self, gpu: &BenchGpu, scene: &ClipScene) {
        self.odd = !self.odd;
        self.window.ui_mut().theme_mut().window_bg = if self.odd {
            RgbaF32::BLACK
        } else {
            RgbaF32::srgb(0.0, 0.0, 0.01)
        };
        let paint = self.window.frame(|ui| scene.record(ui)).paint();
        let arm = scene.arm.label();
        assert_eq!(paint, FramePaint::Full, "clip scene {arm}");
        assert_eq!(
            self.window.render_buffer().rounded_clips.len(),
            scene.arm.masks(),
            "clip scene {arm}: its clip pool"
        );
        gpu.wait();
        gpu.poll();
    }
}

pub(crate) fn atomic(c: &mut Criterion, backends: &Backends) {
    clip_pass(c, backends);
    clip_pass_batch(c, backends);
}

/// `atomic/clip_pass/gpu/<arm>`: see the module doc.
fn clip_pass(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "clip_pass", Scale::Frame);
    for arm in ClipArm::ALL {
        group.bench_function(backend.case(arm.label()), |b| {
            let gpu = backends.device(Timing::PassOnly);
            let mut frames = WholeFrames::new(gpu);
            let scene = ClipScene::new(frames.window.ui(), arm);
            let mut time = || {
                frames.frame(gpu, &scene);
                frames.window.ui().gpu_pass_stats().last_pass()
            };
            for _ in 0..SETTLE_FRAMES {
                time();
            }
            fixture::settle_timestamps(&mut time);
            b.iter_custom(|iters| fixture::device_time(iters, &mut time));
        });
    }
    group.finish();
}

/// `atomic/clip_pass_batch/gpu/<kind>/<arm>`: see the module doc.
fn clip_pass_batch(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.batch_timed_gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "clip_pass_batch", Scale::Frame);
    let kinds = [
        BatchKind::Quads,
        BatchKind::Text,
        BatchKind::Image,
        BatchKind::Curve,
    ];
    for kind in kinds {
        for arm in ClipArm::ALL {
            let case = format!("{}/{}", kind.label(), arm.label());
            group.bench_function(backend.case(&case), |b| {
                let gpu = backends.device(Timing::Instrumented);
                let mut frames = WholeFrames::new(gpu);
                let scene = ClipScene::new(frames.window.ui(), arm);
                let mut time = || {
                    frames.frame(gpu, &scene);
                    frames.window.ui().gpu_pass_stats().last_kind(kind)
                };
                for _ in 0..SETTLE_FRAMES {
                    time();
                }
                fixture::settle_timestamps(&mut time);
                b.iter_custom(|iters| fixture::device_time(iters, &mut time));
            });
        }
    }
    group.finish();
}

/// `integration/clip_frame/<backend>/<arm>/<frame>`: see the module doc.
pub(crate) fn integration(c: &mut Criterion, backends: &Backends) {
    let mut group = Kind::Integration.group(c, "clip_frame", Scale::Frame);
    for backend in backends.all() {
        for arm in ClipArm::ALL {
            for frame in FrameArm::ALL {
                let case = format!("{}/{}", arm.label(), frame.label());
                match backend {
                    Backend::Cpu => cpu_frames(&mut group, backend, arm, frame, &case),
                    Backend::Gpu => gpu_frames(&mut group, backends, arm, frame, &case),
                }
            }
        }
    }
    group.finish();
}

fn cpu_frames(
    group: &mut CaseGroup<'_>,
    backend: Backend,
    arm: ClipArm,
    frame: FrameArm,
    case: &str,
) {
    group.bench_function(backend.case(case), |b| {
        let mut h = window(HarnessDepth::Compose, false);
        let mut scene = ClipScene::new(h.ui(), arm);
        let mut step = || {
            scene.advance(frame);
            let paint = h.frame(|ui| scene.record(ui)).paint();
            (paint, h.render_buffer().rounded_clips.len())
        };
        for _ in 0..SETTLE_FRAMES {
            step();
        }
        let (paint, masks) = step();
        assert_eq!(paint, frame.paint(), "clip_frame {case}");
        assert_eq!(masks, frame.masks(arm), "clip_frame {case}: its clip pool");
        b.iter(&mut step);
    });
}

fn gpu_frames(
    group: &mut CaseGroup<'_>,
    backends: &Backends,
    arm: ClipArm,
    frame: FrameArm,
    case: &str,
) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    group.bench_function(backend.case(case), |b| {
        let gpu = backends.device(Timing::PassOnly);
        let mut window = window(HarnessDepth::gpu(gpu.handles()), false);
        let mut scene = ClipScene::new(window.ui(), arm);
        let mut step = || {
            scene.advance(frame);
            let paint = window.frame(|ui| scene.record(ui)).paint();
            gpu.wait();
            (paint, window.render_buffer().rounded_clips.len())
        };
        for _ in 0..SETTLE_FRAMES {
            step();
        }
        let (paint, masks) = step();
        assert_eq!(paint, frame.paint(), "clip_frame {case}");
        assert_eq!(masks, frame.masks(arm), "clip_frame {case}: its clip pool");
        b.iter(&mut step);
    });
}
