//! The backdrop's device benches, each in device time between passes. Every frame moves the
//! clear, so each paints whole and captures once.
//!
//! - `atomic/backdrop_capture/gpu/<size>`: the capture of a pane over the left half of the frame
//!   fixture, scrolled a pixel a frame: the copy of its footprint out of the target. A pane over
//!   the whole surface would paint through the backbuffer and copy nothing.
//! - `atomic/backdrop_composite/gpu/<look>/<side>`: the main pass of a `side`² surface holding one
//!   rect and a pane over all of it, less the pane's capture and blur between its two passes: the
//!   pane's composite, with the clear, the rect and the pass break beside it.
//! - `integration/backdrop_frame/<backend>/<look>/<arm>`: whole frames under three panes; see
//!   [`integration`].
//! - `integration/backdrop_frame/gpu/<scene>/<arm>`: whole frames of the panes' other
//!   arrangements: a modal, cards, a first pane, a stacked pair, near and apart pairs, and more
//!   stale panes than a partial frame holds; see [`scenes`].

use crate::bench::backends::{Backend, Backends};
use crate::bench::case_group::CaseGroup;
use crate::bench::fixture;
use crate::bench::kind::{Kind, Scale};
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::host::offscreen::OffscreenHost;
use crate::internals::frame_fixture::{BENCH_SCALE, FrameFixture};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::{BackdropFilter, Lens};
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::rect::RectShape;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use criterion::{Bencher, Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::time::{Duration, Instant};

const SIZES: [UVec2; 2] = [UVec2::new(1920, 1080), UVec2::new(3840, 2160)];
/// The pane's σ in physical px: two levels down, so the blur after the capture stays small.
const SIGMA: f32 = 10.0;

/// What a frame records: the frame fixture, shifted by `scroll`, under a pane over the left half
/// of the surface.
#[derive(Debug, Default)]
struct State {
    fixture: FrameFixture,
    scroll: f32,
}

impl State {
    fn record(&mut self, ui: &mut Ui) {
        Panel::zstack()
            .id_salt("backdrop-capture-bench")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                self.fixture.scroll_offset = Vec2::new(self.scroll, 0.0);
                self.fixture.render(BENCH_SCALE, ui);
                Panel::hstack()
                    .id_salt("backdrop-capture-bench-row")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::canvas()
                            .id_salt("backdrop-capture-bench-pane")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                ui.add_shape(
                                    Shape::owner_rect()
                                        .backdrop_filter(BackdropFilter::blur(SIGMA)),
                                );
                            });
                        Panel::canvas()
                            .id_salt("backdrop-capture-bench-rest")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |_| {});
                    });
            });
    }
}

pub(crate) fn atomic(c: &mut Criterion, backends: Backends) {
    let Some(backend) = backends.encoder_timed_gpu() else {
        return;
    };
    capture(c, backend);
    composite(c, backend);
}

/// A host on the pass-timed device whose every frame paints whole.
#[derive(Debug)]
struct WholeFrames {
    host: OffscreenHost,
    odd: bool,
}

impl WholeFrames {
    fn new(gpu: &BenchGpu) -> Self {
        Self {
            host: OffscreenHost::builder(gpu.handles())
                .collect_gpu_stats(true)
                .swapchain_present()
                .build(),
            odd: false,
        }
    }

    /// One frame of `record` on `target`, under a clear unlike the last one's, waited for.
    fn frame(&mut self, gpu: &BenchGpu, target: &OwnedTarget, record: impl FnMut(&mut Ui)) {
        self.odd = !self.odd;
        self.host.ui().theme_mut().window_clear = if self.odd {
            RgbaF32::BLACK
        } else {
            RgbaF32::srgb(0.0, 0.0, 0.01)
        };
        let mut app = record;
        let paint = self.host.frame(target.as_target(), 1.0, &mut app).paint();
        assert_eq!(
            paint,
            FramePaint::Full,
            "a frame with a new clear paints whole"
        );
        gpu.wait();
        gpu.poll();
    }
}

fn capture(c: &mut Criterion, backend: Backend) {
    let mut group = Kind::Atomic.group(c, "backdrop_capture", Scale::Frame);
    for size in SIZES {
        group.throughput(Throughput::Elements(u64::from(size.x * size.y)));
        let case = format!("{}x{}", size.x, size.y);
        group.bench_function(backend.case(&case), |bencher| {
            let gpu = BenchGpu::shared(Timing::PassOnly);
            let target = gpu.target("palantir.bench.backdrop_capture", size);
            let mut frames = WholeFrames::new(gpu);
            let mut state = State::default();
            let mut frame = || {
                state.scroll = (state.scroll + 1.0) % 64.0;
                frames.frame(gpu, &target, |ui| state.record(ui));
                let times = frames.host.gpu_pass_stats().last_backdrops();
                times.captures.map(Duration::from_nanos)
            };
            fixture::settle_timestamps(&mut frame);
            bencher.iter_custom(|iters| fixture::device_time(iters, &mut frame));
        });
    }
    group.finish();
}

#[derive(Clone, Copy, Debug)]
enum Look {
    Blur,
    Frosted,
    /// Glass whose red and blue part at the rim.
    Glass,
    /// Glass of one index for every color.
    GlassPlain,
}

impl Look {
    const ALL: [Self; 4] = [Self::Blur, Self::Frosted, Self::Glass, Self::GlassPlain];
    /// The looks the frame bench runs: one each.
    const FRAME: [Self; 3] = [Self::Blur, Self::Frosted, Self::Glass];

    const fn label(self) -> &'static str {
        match self {
            Self::Blur => "blur",
            Self::Frosted => "frosted",
            Self::Glass => "glass",
            Self::GlassPlain => "glass_plain",
        }
    }

    /// The look's backdrop at σ `sigma` logical px, and its tint.
    fn backdrop(self, sigma: f32) -> (BackdropFilter, RgbaF32) {
        let lens = |dispersion| {
            Some(Lens {
                bezel: 24.0,
                thickness: 16.0,
                refractive_index: 1.5,
                dispersion,
                highlight: 0.6,
            })
        };
        let backdrop = BackdropFilter::blur(sigma);
        match self {
            Self::Blur => (backdrop, RgbaF32::TRANSPARENT),
            Self::Frosted => (
                BackdropFilter {
                    saturation: 1.8,
                    noise: 0.04,
                    ..backdrop
                },
                RgbaF32::new(1.0, 1.0, 1.0, 0.2),
            ),
            Self::Glass => (
                BackdropFilter {
                    blur: 2.0,
                    lens: lens(0.06),
                    ..backdrop
                },
                RgbaF32::TRANSPARENT,
            ),
            Self::GlassPlain => (
                BackdropFilter {
                    blur: 2.0,
                    lens: lens(0.0),
                    ..backdrop
                },
                RgbaF32::TRANSPARENT,
            ),
        }
    }

    fn pane(self) -> RectShape {
        let (backdrop, tint) = self.backdrop(SIGMA);
        Shape::owner_rect().fill(tint).backdrop_filter(backdrop)
    }
}

fn composite(c: &mut Criterion, backend: Backend) {
    let mut group = Kind::Atomic.group(c, "backdrop_composite", Scale::Frame);
    for look in Look::ALL {
        for side in [512, 2048] {
            group.throughput(Throughput::Elements(u64::from(side * side)));
            let case = format!("{}/{side}", look.label());
            group.bench_function(backend.case(&case), |bencher| {
                let gpu = BenchGpu::shared(Timing::PassOnly);
                let target = gpu.target("palantir.bench.backdrop_composite", UVec2::splat(side));
                let mut frames = WholeFrames::new(gpu);
                let mut frame = || {
                    frames.frame(gpu, &target, |ui| {
                        Panel::canvas()
                            .id_salt("backdrop-composite-bench")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                ui.add_shape(
                                    Shape::rect(Rect::new(10.0, 10.0, 100.0, 100.0))
                                        .fill(RgbaF32::WHITE),
                                );
                                ui.add_shape(look.pane());
                            });
                    });
                    let host = &frames.host;
                    assert_eq!(host.last_render_buffer().backdrops.len(), 1, "one pane");
                    // The main pass spans the pane's capture and blur, which `backdrop_capture`
                    // and `backdrop_blur` time.
                    let stats = host.gpu_pass_stats();
                    let backdrop = stats.last_backdrops();
                    let between = backdrop.captures? + backdrop.blurs?;
                    Some(stats.last_pass()? - Duration::from_nanos(between))
                };
                fixture::settle_timestamps(&mut frame);
                bencher.iter_custom(|iters| fixture::device_time(iters, &mut frame));
            });
        }
    }
    group.finish();
}

/// What changes between two frames of one arm of `integration/backdrop_frame`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    /// Nothing: every frame skips.
    Cached,
    /// A square above the toolbar's pane, inside it, flips color: no pane goes stale.
    Hover,
    /// A square under the toolbar's pane flips color: that pane captures again, and the sidebar's,
    /// whose footprint the toolbar's rect reaches.
    Under,
    /// The popup's pane steps a pixel and back inside the region it captured.
    Sliding,
    /// The popup's pane moves 4 logical px a frame, out of the region it captured, as an
    /// animated popup does.
    Moving,
    /// The page scrolls under every pane: each captures again.
    Scrolling,
}

impl Arm {
    const ALL: [Self; 6] = [
        Self::Cached,
        Self::Hover,
        Self::Under,
        Self::Sliding,
        Self::Moving,
        Self::Scrolling,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Cached => "cached",
            Self::Hover => "hover",
            Self::Under => "under",
            Self::Sliding => "sliding",
            Self::Moving => "moving",
            Self::Scrolling => "scrolling",
        }
    }

    /// The paint a settled frame of `look` plans, and on the device the blurs it runs. A slide
    /// keeps its capture only on a lattice coarser than a pixel, which glass's light blur is not.
    fn expected(self, look: Look) -> (FramePaint, usize) {
        let sigma = look.backdrop(FRAME_SIGMA).0.blur * FRAME_DPR;
        let slides_inside = BlurPlan::new(sigma).lattice() > 1;
        match self {
            Self::Cached => (FramePaint::Skip, 0),
            Self::Hover => (FramePaint::Partial, 0),
            Self::Sliding => (FramePaint::Partial, usize::from(!slides_inside)),
            Self::Moving => (FramePaint::Partial, 1),
            Self::Under => (FramePaint::Partial, 2),
            Self::Scrolling => (FramePaint::Full, 3),
        }
    }
}

/// The surface of `integration/backdrop_frame`, a 1280 × 720 window at 2×.
const FRAME_SURFACE: UVec2 = UVec2::new(2560, 1440);
const FRAME_DPR: f32 = 2.0;
/// The frame bench's σ in logical px, where a look blurs at its own.
const FRAME_SIGMA: f32 = 16.0;
const TOOLBAR: Rect = Rect::new(0.0, 0.0, 1280.0, 48.0);
const SIDEBAR: Rect = Rect::new(0.0, 48.0, 240.0, 672.0);
const POPUP: Rect = Rect::new(500.0, 200.0, 320.0, 220.0);

/// The frame fixture with a toolbar, a sidebar and a popup as panes over it, in `look`, at frame
/// `n` of `arm`.
#[derive(Debug, Default)]
struct PaneFrame {
    fixture: FrameFixture,
    n: u32,
}

impl PaneFrame {
    fn record(&mut self, look: Look, arm: Arm, ui: &mut Ui) {
        let odd = self.n % 2 == 1;
        let flip = |on: bool| {
            if on && odd {
                RgbaF32::srgb(1.0, 0.3, 0.1)
            } else {
                RgbaF32::srgb(0.1, 0.3, 1.0)
            }
        };
        if arm == Arm::Scrolling {
            self.fixture.scroll_offset = Vec2::new(0.0, (self.n % 64) as f32);
        }
        let popup = match arm {
            Arm::Sliding if odd => Rect {
                min: POPUP.min + Vec2::X,
                ..POPUP
            },
            Arm::Moving => Rect {
                min: POPUP.min + Vec2::new(4.0 * (self.n % 32) as f32, 0.0),
                ..POPUP
            },
            _ => POPUP,
        };
        Panel::zstack()
            .id_salt("backdrop-frame")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                self.fixture.render(BENCH_SCALE, ui);
                Panel::canvas()
                    .id_salt("backdrop-frame-panes")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::rect(Rect::new(300.0, 20.0, 12.0, 12.0))
                                .fill(flip(arm == Arm::Under)),
                        );
                        for rect in [TOOLBAR, SIDEBAR, popup] {
                            ui.add_shape(look.pane_at(rect));
                        }
                        ui.add_shape(
                            Shape::rect(Rect::new(400.0, 20.0, 12.0, 12.0))
                                .fill(flip(arm == Arm::Hover)),
                        );
                    });
            });
    }
}

impl Look {
    fn pane_at(self, rect: Rect) -> RectShape {
        let (backdrop, tint) = self.backdrop(FRAME_SIGMA);
        Shape::rect(rect).fill(tint).backdrop_filter(backdrop)
    }
}

/// `integration/backdrop_frame/<backend>/<look>/<arm>`: one whole frame of the frame fixture under
/// three panes, wall time. `cpu` records through compose with no device; `gpu` runs the host's
/// frame on the desktop's present strategy and waits for it. Each case checks its arm's paint, and
/// on the device the blurs its frames run.
pub(crate) fn integration(c: &mut Criterion, backends: Backends) {
    let mut group = Kind::Integration.group(c, "backdrop_frame", Scale::Frame);
    for backend in backends.all() {
        for look in Look::FRAME {
            for arm in Arm::ALL {
                let case = backend.case(&format!("{}/{}", look.label(), arm.label()));
                group.bench_function(case, |bencher| match backend {
                    Backend::Cpu => {
                        let mut h = Harness::builder(FRAME_SURFACE)
                            .scale(FRAME_DPR)
                            .depth(Depth::Compose)
                            .build();
                        let start = Instant::now();
                        let mut state = PaneFrame::default();
                        let mut frame = || {
                            state.n += 1;
                            h.at(start.elapsed());
                            h.frame(|ui| state.record(look, arm, ui)).paint()
                        };
                        for _ in 0..4 {
                            frame();
                        }
                        assert_eq!(frame(), arm.expected(look).0, "{}", arm.label());
                        bencher.iter(&mut frame);
                    }
                    Backend::Gpu => pane_frames(bencher, look, arm),
                });
            }
        }
    }
    if let Some(backend) = backends.gpu() {
        scenes(&mut group, backend);
    }
    group.finish();
}

/// The device half of `integration/backdrop_frame`.
fn pane_frames(bencher: &mut Bencher<'_>, look: Look, arm: Arm) {
    let gpu = BenchGpu::shared(Timing::PassOnly);
    let target = gpu.target("palantir.bench.backdrop_frame", FRAME_SURFACE);
    let mut host = OffscreenHost::builder(gpu.handles())
        .collect_gpu_stats(false)
        .swapchain_present()
        .build();
    let mut state = PaneFrame::default();
    let mut frame = || {
        state.n += 1;
        let mut app = |ui: &mut Ui| state.record(look, arm, ui);
        let paint = host.frame(target.as_target(), FRAME_DPR, &mut app).paint();
        gpu.wait();
        (paint, host.backdrop_blurs())
    };
    for _ in 0..4 {
        frame();
    }
    let (paint, blurs) = frame();
    let (want_paint, want_blurs) = arm.expected(look);
    assert_eq!(paint, want_paint, "{}", arm.label());
    if paint != FramePaint::Skip {
        assert_eq!(blurs, want_blurs, "{}: blurs", arm.label());
    }
    bencher.iter(&mut frame);
}

/// The 4K surface of the modal arms, a 1920 × 1080 window at 2×.
const MODAL_SURFACE: UVec2 = UVec2::new(3840, 2160);

/// One case of `integration/backdrop_frame/gpu` past the three panes: what its frames record,
/// and what a settled frame paints and blurs.
#[derive(Clone, Copy, Debug)]
struct SceneCase {
    name: &'static str,
    surface: UVec2,
    want: (FramePaint, usize),
    /// Frame `n` over the frame fixture.
    record: fn(&mut FrameFixture, u32, &mut Ui),
}

impl SceneCase {
    /// Every scene case, in the order the bench runs them.
    const ALL: [Self; 9] = [
        Self {
            name: "modal/cached",
            surface: MODAL_SURFACE,
            want: (FramePaint::Skip, 0),
            record: |fixture, _, ui| modal(fixture, ui),
        },
        Self {
            name: "modal/scrolling",
            surface: MODAL_SURFACE,
            want: (FramePaint::Full, 1),
            record: |fixture, n, ui| {
                scroll(fixture, n);
                modal(fixture, ui);
            },
        },
        Self {
            name: "cards/scrolling",
            surface: FRAME_SURFACE,
            want: (FramePaint::Full, 8),
            record: |fixture, n, ui| {
                scroll(fixture, n);
                over(fixture, ui, |ui| {
                    for i in 0..8 {
                        let at =
                            Vec2::new(60.0 + (i % 4) as f32 * 300.0, 80.0 + (i / 4) as f32 * 260.0);
                        ui.add_shape(Look::Frosted.pane_at(Rect::new(at.x, at.y, 240.0, 200.0)));
                    }
                });
            },
        },
        Self {
            name: "cards_apart/scrolling",
            surface: FRAME_SURFACE,
            want: (FramePaint::Full, 6),
            record: |fixture, n, ui| {
                scroll(fixture, n);
                over(fixture, ui, |ui| {
                    for i in 0..6 {
                        let at =
                            Vec2::new(40.0 + (i % 3) as f32 * 400.0, 60.0 + (i / 3) as f32 * 320.0);
                        ui.add_shape(Look::Frosted.pane_at(Rect::new(at.x, at.y, 300.0, 220.0)));
                    }
                });
            },
        },
        Self {
            name: "first/scrolling",
            surface: FRAME_SURFACE,
            want: (FramePaint::Full, 0),
            record: |fixture, n, ui| {
                scroll(fixture, n);
                first(fixture, ui);
            },
        },
        Self {
            name: "stacked/scrolling",
            surface: FRAME_SURFACE,
            want: (FramePaint::Full, 2),
            record: |fixture, n, ui| {
                scroll(fixture, n);
                over(fixture, ui, |ui| {
                    ui.add_shape(Look::Frosted.pane_at(SIDEBAR));
                    ui.add_shape(Look::Glass.pane_at(Rect::new(160.0, 200.0, 320.0, 220.0)));
                });
            },
        },
        Self {
            name: "pair_near/scrolling",
            surface: FRAME_SURFACE,
            want: (FramePaint::Full, 2),
            record: |fixture, n, ui| {
                scroll(fixture, n);
                pair(fixture, ui, 8.0);
            },
        },
        Self {
            name: "pair_apart/scrolling",
            surface: FRAME_SURFACE,
            want: (FramePaint::Full, 2),
            record: |fixture, n, ui| {
                scroll(fixture, n);
                pair(fixture, ui, 160.0);
            },
        },
        Self {
            name: "many/under",
            surface: FRAME_SURFACE,
            want: (FramePaint::Full, 12),
            record: |fixture, n, ui| {
                over(fixture, ui, |ui| {
                    let color = if n % 2 == 1 {
                        RgbaF32::srgb(1.0, 0.3, 0.1)
                    } else {
                        RgbaF32::srgb(0.1, 0.3, 1.0)
                    };
                    for y in [150.0, 400.0] {
                        ui.add_shape(Shape::rect(Rect::new(40.0, y, 1200.0, 12.0)).fill(color));
                    }
                    for i in 0..12 {
                        let at = Vec2::new(
                            40.0 + (i % 6) as f32 * 205.0,
                            100.0 + (i / 6) as f32 * 250.0,
                        );
                        ui.add_shape(Look::Frosted.pane_at(Rect::new(at.x, at.y, 160.0, 120.0)));
                    }
                });
            },
        },
    ];
}

/// Scroll the page of `fixture` to frame `n`'s place.
const fn scroll(fixture: &mut FrameFixture, n: u32) {
    fixture.scroll_offset = Vec2::new(0.0, (n % 64) as f32);
}

/// The frame fixture, and `panes` on a canvas over it.
fn over(fixture: &mut FrameFixture, ui: &mut Ui, panes: impl FnOnce(&mut Ui)) {
    Panel::zstack()
        .id_salt("backdrop-scene")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            fixture.render(BENCH_SCALE, ui);
            Panel::canvas()
                .id_salt("backdrop-scene-panes")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, panes);
        });
}

/// A frosted pane over the whole window, as a modal dialog's page sits.
fn modal(fixture: &mut FrameFixture, ui: &mut Ui) {
    over(fixture, ui, |ui| ui.add_shape(Look::Frosted.pane()));
}

/// Two frosted cards side by side, `gap` logical px apart: at 8 px the second's reach covers the
/// first, at 160 px it covers nothing painted after the page.
fn pair(fixture: &mut FrameFixture, ui: &mut Ui, gap: f32) {
    over(fixture, ui, |ui| {
        for i in 0..2 {
            let x = 200.0 + i as f32 * (320.0 + gap);
            ui.add_shape(Look::Frosted.pane_at(Rect::new(x, 160.0, 320.0, 240.0)));
        }
    });
}

/// A window background, which folds into the clear, then a frosted sidebar over it with nothing
/// painted before it, then the page beside the sidebar.
fn first(fixture: &mut FrameFixture, ui: &mut Ui) {
    Panel::zstack()
        .id_salt("backdrop-first")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::canvas()
                .id_salt("backdrop-first-background")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::owner_rect().fill(RgbaF32::srgb(0.12, 0.13, 0.16)));
                });
            Panel::hstack()
                .id_salt("backdrop-first-row")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas()
                        .id_salt("backdrop-first-sidebar")
                        .size((Sizing::fixed(240.0), Sizing::FILL))
                        .show(ui, |ui| ui.add_shape(Look::Frosted.pane()));
                    fixture.render(BENCH_SCALE, ui);
                });
        });
}

/// `integration/backdrop_frame/gpu/<scene>/<arm>`: one whole frame of each [`SceneCase`], wall
/// time, at 2×, waited for. Each case checks its settled frame's paint and blurs.
///
/// - `modal`: the frame fixture at 4K under a frosted pane over the window; `cached` keeps every
///   frame still, `scrolling` scrolls the page under the pane, which captures every frame.
/// - `cards`: eight frosted cards in a 4 × 2 grid, 60 logical px apart, as a dashboard of glass
///   tiles sits: inside the 77.5 logical px a σ of 16 reaches, so each card's footprint takes in
///   its neighbors'.
/// - `cards_apart`: six frosted cards in a 3 × 2 grid, 100 logical px apart, past that reach.
/// - `first`: a frosted sidebar with nothing painted before it, which shows the clear, beside the
///   scrolling page.
/// - `stacked`: a glass popup over a frosted sidebar, which captures what the sidebar painted.
/// - `pair_near` and `pair_apart`: two frosted cards whose gap the second's reach covers or not.
/// - `many/under`: a strip under each row of twelve frosted cards flips color, staling more panes
///   than a partial frame holds, so the frame paints whole.
fn scenes(group: &mut CaseGroup<'_>, backend: Backend) {
    for scene in SceneCase::ALL {
        group.bench_function(backend.case(scene.name), |bencher| {
            let gpu = BenchGpu::shared(Timing::PassOnly);
            let target = gpu.target("palantir.bench.backdrop_scene", scene.surface);
            let mut host = OffscreenHost::builder(gpu.handles())
                .collect_gpu_stats(false)
                .swapchain_present()
                .build();
            let mut fixture = FrameFixture::default();
            let mut n = 0u32;
            let mut frame = || {
                n += 1;
                let mut app = |ui: &mut Ui| (scene.record)(&mut fixture, n, ui);
                let paint = host.frame(target.as_target(), FRAME_DPR, &mut app).paint();
                gpu.wait();
                (paint, host.backdrop_blurs())
            };
            for _ in 0..4 {
                frame();
            }
            let (paint, blurs) = frame();
            assert_eq!(paint, scene.want.0, "{}", scene.name);
            if paint != FramePaint::Skip {
                assert_eq!(blurs, scene.want.1, "{}: blurs", scene.name);
            }
            bencher.iter(&mut frame);
        });
    }
}
