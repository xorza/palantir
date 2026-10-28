//! The backdrop's device benches, each in device time between passes. Every frame moves the
//! clear, so each paints whole and captures once.
//!
//! - `atomic/backdrop_capture/gpu/<size>`: the capture of a whole-surface pane over the frame
//!   fixture, scrolled a pixel a frame: the copy of its footprint out of the target.
//! - `atomic/backdrop_composite/gpu/<look>/<side>`: the main pass of a `side`² surface holding one
//!   rect and a pane over all of it, less the pane's capture and blur between its two passes: the
//!   pane's composite, with the clear, the rect and the pass break beside it.
//! - `integration/backdrop_frame/<backend>/<look>/<arm>`: whole frames under three panes; see
//!   [`integration`].
//! - `integration/backdrop_frame/gpu/modal/<arm>`: whole 4K frames under a pane over the window;
//!   see [`modal`].
//! - `integration/backdrop_frame/gpu/cards/scrolling`: whole frames under eight frosted cards apart
//!   from each other; see [`cards`].

use crate::bench::backends::{Backend, Backends};
use crate::bench::case_group::CaseGroup;
use crate::bench::fixture;
use crate::bench::kind::{Kind, Scale};
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::host::offscreen::OffscreenHost;
use crate::internals::frame_fixture::{BENCH_SCALE, FrameFixture};
use crate::internals::record_app::RecordApp;
use crate::internals::ui_harness::UiHarness;
use crate::internals::ui_harness::frontend_harness::FrontendHarness;
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

/// What a frame records: the frame fixture, shifted by `scroll`, under a pane over the whole
/// surface.
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
                Panel::canvas()
                    .id_salt("backdrop-capture-bench-pane")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::owner_rect().backdrop_filter(BackdropFilter::blur(SIGMA)),
                        );
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
                .retained_target(true)
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
        let mut app = RecordApp::new(record);
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
        let backdrop = BackdropFilter {
            blur: sigma,
            saturation: 1.0,
            noise: 0.0,
            lens: None,
        };
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
    /// The page scrolls under every pane: each captures again.
    Scrolling,
}

impl Arm {
    const ALL: [Self; 5] = [
        Self::Cached,
        Self::Hover,
        Self::Under,
        Self::Sliding,
        Self::Scrolling,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Cached => "cached",
            Self::Hover => "hover",
            Self::Under => "under",
            Self::Sliding => "sliding",
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
        let popup = if arm == Arm::Sliding && odd {
            Rect {
                min: POPUP.min + Vec2::X,
                ..POPUP
            }
        } else {
            POPUP
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
                        let mut h = FrontendHarness::new(
                            UiHarness::with_text(FRAME_SURFACE).scale(FRAME_DPR),
                        );
                        let start = Instant::now();
                        let mut state = PaneFrame::default();
                        let mut frame = || {
                            state.n += 1;
                            h.harness().at(start.elapsed());
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
        modal(&mut group, backend);
        cards(&mut group, backend);
    }
    group.finish();
}

/// The device half of `integration/backdrop_frame`.
fn pane_frames(bencher: &mut Bencher<'_>, look: Look, arm: Arm) {
    let gpu = BenchGpu::shared(Timing::PassOnly);
    let target = gpu.target("palantir.bench.backdrop_frame", FRAME_SURFACE);
    let mut host = OffscreenHost::builder(gpu.handles())
        .collect_gpu_stats(false)
        .retained_target(true)
        .build();
    let mut state = PaneFrame::default();
    let mut frame = || {
        state.n += 1;
        let mut app = RecordApp::new(|ui| state.record(look, arm, ui));
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

/// `integration/backdrop_frame/gpu/modal/<arm>`: one whole frame of the frame fixture at 4K under
/// a frosted pane over the whole window, as a modal dialog's page sits; `cached` keeps every frame
/// still, `scrolling` scrolls the page under the pane, which captures every frame.
fn modal(group: &mut CaseGroup<'_>, backend: Backend) {
    for arm in [Arm::Cached, Arm::Scrolling] {
        let case = backend.case(&format!("modal/{}", arm.label()));
        group.bench_function(case, |bencher| {
            let gpu = BenchGpu::shared(Timing::PassOnly);
            let target = gpu.target("palantir.bench.backdrop_modal", MODAL_SURFACE);
            let mut host = OffscreenHost::builder(gpu.handles())
                .collect_gpu_stats(false)
                .retained_target(true)
                .build();
            let mut fixture = FrameFixture::default();
            let mut n = 0u32;
            let mut frame = || {
                n += 1;
                if arm == Arm::Scrolling {
                    fixture.scroll_offset = Vec2::new(0.0, (n % 64) as f32);
                }
                let mut app = RecordApp::new(|ui: &mut Ui| {
                    Panel::zstack()
                        .id_salt("backdrop-modal")
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            fixture.render(BENCH_SCALE, ui);
                            Panel::canvas()
                                .id_salt("backdrop-modal-pane")
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, |ui| {
                                    ui.add_shape(Look::Frosted.pane());
                                });
                        });
                });
                let paint = host.frame(target.as_target(), FRAME_DPR, &mut app).paint();
                gpu.wait();
                (paint, host.backdrop_blurs())
            };
            for _ in 0..4 {
                frame();
            }
            let (paint, blurs) = frame();
            match arm {
                Arm::Cached => assert_eq!(paint, FramePaint::Skip, "a still modal skips"),
                _ => assert_eq!((paint, blurs), (FramePaint::Full, 1), "the page scrolls"),
            }
            bencher.iter(&mut frame);
        });
    }
}

/// `integration/backdrop_frame/gpu/cards/scrolling`: whole frames of the frame fixture scrolling
/// under eight frosted cards in a 4 × 2 grid, 60 logical px apart, past a blur's reach, as a
/// dashboard of glass tiles sits. Every card captures every frame, and none paints under another's
/// footprint.
fn cards(group: &mut CaseGroup<'_>, backend: Backend) {
    group.bench_function(backend.case("cards/scrolling"), |bencher| {
        let gpu = BenchGpu::shared(Timing::PassOnly);
        let target = gpu.target("palantir.bench.backdrop_cards", FRAME_SURFACE);
        let mut host = OffscreenHost::builder(gpu.handles())
            .collect_gpu_stats(false)
            .retained_target(true)
            .build();
        let mut fixture = FrameFixture::default();
        let mut n = 0u32;
        let mut frame = || {
            n += 1;
            fixture.scroll_offset = Vec2::new(0.0, (n % 64) as f32);
            let mut app = RecordApp::new(|ui: &mut Ui| {
                Panel::zstack()
                    .id_salt("backdrop-cards")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        fixture.render(BENCH_SCALE, ui);
                        Panel::canvas()
                            .id_salt("backdrop-cards-panes")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                for i in 0..8 {
                                    let at = Vec2::new(
                                        60.0 + (i % 4) as f32 * 300.0,
                                        80.0 + (i / 4) as f32 * 260.0,
                                    );
                                    let rect = Rect::new(at.x, at.y, 240.0, 200.0);
                                    ui.add_shape(Look::Frosted.pane_at(rect));
                                }
                            });
                    });
            });
            let paint = host.frame(target.as_target(), FRAME_DPR, &mut app).paint();
            gpu.wait();
            (paint, host.backdrop_blurs())
        };
        for _ in 0..4 {
            frame();
        }
        assert_eq!(
            frame(),
            (FramePaint::Full, 8),
            "the page scrolls under every card"
        );
        bencher.iter(&mut frame);
    });
}
