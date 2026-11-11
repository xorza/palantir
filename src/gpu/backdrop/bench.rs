//! The backdrop's device benches, each in device time between passes. Every frame moves the
//! clear, so each paints whole and captures once.
//!
//! - `atomic/backdrop_capture/gpu/<size>`: the capture of a backdrop over the left half of the
//!   scene, scrolled a pixel a frame: the copy of its footprint out of the target. A backdrop over
//!   the whole surface would paint through the backbuffer and copy nothing.
//! - `atomic/backdrop_composite/gpu/<look>/<side>`: the main pass of a `side`² surface holding one
//!   rect and a backdrop over all of it, less the backdrop's capture and blur between its two
//!   passes: the backdrop's composite, with the clear, the rect and the pass break beside it.
//! - `integration/backdrop_frame/<backend>/<look>/<arm>`: whole frames under three backdrops; see
//!   [`integration`].
//! - `integration/backdrop_frame/gpu/<scene>/<arm>`: whole frames of the backdrops' other
//!   arrangements: a modal, cards, a first backdrop, a stacked pair, near and apart pairs, and more
//!   stale backdrops than a partial frame holds; see [`scenes`].

use crate::bench::backends::{Backend, Backends};
use crate::bench::case_group::CaseGroup;
use crate::bench::fixture;
use crate::bench::kind::{Kind, Scale};
use crate::bench::scene::{Scene, SceneFrame};
use crate::display::system_scale::SystemScale;
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::paint::Paint;
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use criterion::{Bencher, Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::time::Instant;

const SIZES: [UVec2; 2] = [UVec2::new(1920, 1080), UVec2::new(3840, 2160)];
/// The backdrop's σ in physical px: two levels down, so the blur after the capture stays small.
const SIGMA: f32 = 10.0;

/// What a frame records: the scene, shifted by `scroll`, under a backdrop over the left half
/// of the surface.
#[derive(Debug, Default)]
struct State<S> {
    frame: SceneFrame<S>,
    scroll: f32,
}

impl<S: Scene> State<S> {
    fn record(&mut self, ui: &mut Ui) {
        Panel::zstack()
            .id_salt("backdrop-capture-bench")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                self.frame.scroll = Vec2::new(self.scroll, 0.0);
                self.frame.record(ui);
                Panel::hstack()
                    .id_salt("backdrop-capture-bench-row")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::canvas()
                            .id_salt("backdrop-capture-bench-backdrop")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                ui.add_shape(Shape::rect(), Material::blur(SIGMA));
                            });
                        Panel::canvas()
                            .id_salt("backdrop-capture-bench-rest")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |_| {});
                    });
            });
    }
}

pub(crate) fn atomic<S: Scene>(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.encoder_timed_gpu() else {
        return;
    };
    capture::<S>(c, backends, backend);
    composite(c, backends, backend);
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
        self.host.ui_mut().theme_mut().window_bg = if self.odd {
            RgbaF32::BLACK
        } else {
            RgbaF32::srgb(0.0, 0.0, 0.01)
        };
        let mut app = record;
        let paint = self.host.frame(target.as_target(), &mut app).paint();
        assert_eq!(
            paint,
            FramePaint::Full,
            "a frame with a new clear paints whole"
        );
        gpu.wait();
        gpu.poll();
    }
}

fn capture<S: Scene>(c: &mut Criterion, backends: &Backends, backend: Backend) {
    let mut group = Kind::Atomic.group(c, "backdrop_capture", Scale::Frame);
    for size in SIZES {
        group.throughput(Throughput::Elements(u64::from(size.x * size.y)));
        let case = format!("{}x{}", size.x, size.y);
        group.bench_function(backend.case(&case), |bencher| {
            let gpu = backends.device(Timing::PassOnly);
            let target = gpu.target("palantir.bench.backdrop_capture", size);
            let mut frames = WholeFrames::new(gpu);
            let mut state = State::<S>::default();
            let mut frame = || {
                state.scroll = (state.scroll + 1.0) % 64.0;
                frames.frame(gpu, &target, |ui| state.record(ui));
                let times = frames.host.ui().gpu_pass_stats().last_backdrops();
                times.captures
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
    /// Glass that spreads the spectrum at its rim.
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

    /// The look's material at σ `sigma` logical px.
    fn material(self, sigma: f32) -> Material {
        let lens = |dispersion| Lens {
            bezel: 24.0,
            thickness: 16.0,
            refractive_index: 1.5,
            dispersion,
            highlight: 0.6,
        };
        match self {
            Self::Blur => Material::blur(sigma),
            Self::Frosted => Material::frosted(sigma),
            Self::Glass => Material::glass(lens(0.06)).with_blur(2.0),
            Self::GlassPlain => Material::glass(lens(0.0)).with_blur(2.0),
        }
    }

    /// The look's paint: its fill over its material, blurred by `sigma` logical px.
    fn paint(self, sigma: f32) -> Paint {
        let fill = match self {
            Self::Frosted => RgbaF32::new(1.0, 1.0, 1.0, 0.2),
            Self::Blur | Self::Glass | Self::GlassPlain => RgbaF32::TRANSPARENT,
        };
        Paint::from(fill).with_material(self.material(sigma))
    }
}

fn composite(c: &mut Criterion, backends: &Backends, backend: Backend) {
    let mut group = Kind::Atomic.group(c, "backdrop_composite", Scale::Frame);
    for look in Look::ALL {
        for side in [512, 2048] {
            group.throughput(Throughput::Elements(u64::from(side * side)));
            let case = format!("{}/{side}", look.label());
            group.bench_function(backend.case(&case), |bencher| {
                let gpu = backends.device(Timing::PassOnly);
                let target = gpu.target("palantir.bench.backdrop_composite", UVec2::splat(side));
                let mut frames = WholeFrames::new(gpu);
                let mut frame = || {
                    frames.frame(gpu, &target, |ui| {
                        Panel::canvas()
                            .id_salt("backdrop-composite-bench")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                ui.add_shape(
                                    Shape::rect().at(Rect::new(10.0, 10.0, 100.0, 100.0)),
                                    RgbaF32::WHITE,
                                );
                                ui.add_shape(Shape::rect(), look.paint(SIGMA));
                            });
                    });
                    let host = &frames.host;
                    assert_eq!(host.last_render_buffer().backdrops.len(), 1, "one backdrop");
                    // The main pass spans the backdrop's capture and blur, which `backdrop_capture`
                    // and `backdrop_blur` time.
                    let stats = host.ui().gpu_pass_stats();
                    let backdrop = stats.last_backdrops();
                    let between = backdrop.captures? + backdrop.blurs?;
                    Some(stats.last_pass()? - between)
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
    /// A square above the toolbar's backdrop, inside it, flips color: no backdrop goes stale.
    Hover,
    /// A square under the toolbar's backdrop flips color: that backdrop captures again, and the
    /// sidebar's, whose footprint the toolbar's rect reaches.
    Under,
    /// The popup's backdrop steps a pixel and back inside the region it captured.
    Sliding,
    /// The popup's backdrop moves 4 logical px a frame, out of the region it captured, as an
    /// animated popup does.
    Moving,
    /// The page scrolls under every backdrop: each captures again.
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
        let sigma = look.material(FRAME_SIGMA).blur * FRAME_DPR.get();
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
const FRAME_DPR: SystemScale = SystemScale::new(2.0).unwrap();
/// The frame bench's σ in logical px, where a look blurs at its own.
const FRAME_SIGMA: f32 = 16.0;
const TOOLBAR: Rect = Rect::new(0.0, 0.0, 1280.0, 48.0);
const SIDEBAR: Rect = Rect::new(0.0, 48.0, 240.0, 672.0);
const POPUP: Rect = Rect::new(500.0, 200.0, 320.0, 220.0);

/// The scene with a toolbar, a sidebar and a popup as backdrops over it, in `look`, at frame
/// `n` of `arm`.
#[derive(Debug, Default)]
struct MaterialFrame<S> {
    frame: SceneFrame<S>,
    n: u32,
}

impl<S: Scene> MaterialFrame<S> {
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
            self.frame.scroll = Vec2::new(0.0, (self.n % 64) as f32);
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
                self.frame.record(ui);
                Panel::canvas()
                    .id_salt("backdrop-frame-backdrops")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::rect().at(Rect::new(300.0, 20.0, 12.0, 12.0)),
                            flip(arm == Arm::Under),
                        );
                        for rect in [TOOLBAR, SIDEBAR, popup] {
                            ui.add_shape(Shape::rect().at(rect), look.paint(FRAME_SIGMA));
                        }
                        ui.add_shape(
                            Shape::rect().at(Rect::new(400.0, 20.0, 12.0, 12.0)),
                            flip(arm == Arm::Hover),
                        );
                    });
            });
    }
}

/// `integration/backdrop_frame/<backend>/<look>/<arm>`: one whole frame of the scene under
/// three backdrops, wall time. `cpu` records through compose with no device; `gpu` runs the host's
/// frame on the desktop's present strategy and waits for it. Each case checks its arm's paint, and
/// on the device the blurs its frames run.
pub(crate) fn integration<S: Scene>(c: &mut Criterion, backends: &Backends) {
    let mut group = Kind::Integration.group(c, "backdrop_frame", Scale::Frame);
    for backend in backends.all() {
        for look in Look::FRAME {
            for arm in Arm::ALL {
                let case = backend.case(&format!("{}/{}", look.label(), arm.label()));
                group.bench_function(case, |bencher| match backend {
                    Backend::Cpu => {
                        let mut h = Harness::builder(FRAME_SURFACE)
                            .system_scale(FRAME_DPR)
                            .depth(Depth::Compose)
                            .build();
                        let start = Instant::now();
                        let mut state = MaterialFrame::<S>::default();
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
                    Backend::Gpu => backdrop_frames::<S>(bencher, backends, look, arm),
                });
            }
        }
    }
    if let Some(backend) = backends.gpu() {
        scenes::<S>(&mut group, backends, backend);
    }
    group.finish();
}

/// The device half of `integration/backdrop_frame`.
fn backdrop_frames<S: Scene>(bencher: &mut Bencher<'_>, backends: &Backends, look: Look, arm: Arm) {
    let gpu = backends.device(Timing::PassOnly);
    let target = gpu.target("palantir.bench.backdrop_frame", FRAME_SURFACE);
    let mut host = OffscreenHost::builder(gpu.handles())
        .collect_gpu_stats(false)
        .swapchain_present()
        .build();
    let mut state = MaterialFrame::<S>::default();
    let mut frame = || {
        state.n += 1;
        let mut app = |ui: &mut Ui| state.record(look, arm, ui);
        let paint = host
            .set_system_scale(FRAME_DPR)
            .frame(target.as_target(), &mut app)
            .paint();
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

/// One case of `integration/backdrop_frame/gpu` past the three backdrops: what its frames record,
/// and what a settled frame paints and blurs.
#[derive(Debug)]
struct SceneCase<S> {
    name: &'static str,
    surface: UVec2,
    want: (FramePaint, usize),
    /// Frame `n` over the scene.
    record: fn(&mut SceneFrame<S>, u32, &mut Ui),
}

impl<S: Scene> SceneCase<S> {
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
                        ui.add_shape(
                            Shape::rect().at(Rect::new(at.x, at.y, 240.0, 200.0)),
                            Look::Frosted.paint(FRAME_SIGMA),
                        );
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
                        ui.add_shape(
                            Shape::rect().at(Rect::new(at.x, at.y, 300.0, 220.0)),
                            Look::Frosted.paint(FRAME_SIGMA),
                        );
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
                    ui.add_shape(Shape::rect().at(SIDEBAR), Look::Frosted.paint(FRAME_SIGMA));
                    ui.add_shape(
                        Shape::rect().at(Rect::new(160.0, 200.0, 320.0, 220.0)),
                        Look::Glass.paint(FRAME_SIGMA),
                    );
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
                        ui.add_shape(Shape::rect().at(Rect::new(40.0, y, 1200.0, 12.0)), color);
                    }
                    for i in 0..12 {
                        let at = Vec2::new(
                            40.0 + (i % 6) as f32 * 205.0,
                            100.0 + (i / 6) as f32 * 250.0,
                        );
                        ui.add_shape(
                            Shape::rect().at(Rect::new(at.x, at.y, 160.0, 120.0)),
                            Look::Frosted.paint(FRAME_SIGMA),
                        );
                    }
                });
            },
        },
    ];
}

/// Scroll the page of `fixture` to frame `n`'s place.
const fn scroll<S>(frame: &mut SceneFrame<S>, n: u32) {
    frame.scroll = Vec2::new(0.0, (n % 64) as f32);
}

/// The scene, and `backdrops` on a canvas over it.
fn over<S: Scene>(fixture: &mut SceneFrame<S>, ui: &mut Ui, backdrops: impl FnOnce(&mut Ui)) {
    Panel::zstack()
        .id_salt("backdrop-scene")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            fixture.record(ui);
            Panel::canvas()
                .id_salt("backdrop-scene-backdrops")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, backdrops);
        });
}

/// A frosted backdrop over the whole window, as a modal dialog's page sits.
fn modal<S: Scene>(fixture: &mut SceneFrame<S>, ui: &mut Ui) {
    over(fixture, ui, |ui| {
        ui.add_shape(Shape::rect(), Look::Frosted.paint(SIGMA));
    });
}

/// Two frosted cards side by side, `gap` logical px apart: at 8 px the second's reach covers the
/// first, at 160 px it covers nothing painted after the page.
fn pair<S: Scene>(fixture: &mut SceneFrame<S>, ui: &mut Ui, gap: f32) {
    over(fixture, ui, |ui| {
        for i in 0..2 {
            let x = 200.0 + i as f32 * (320.0 + gap);
            ui.add_shape(
                Shape::rect().at(Rect::new(x, 160.0, 320.0, 240.0)),
                Look::Frosted.paint(FRAME_SIGMA),
            );
        }
    });
}

/// A window background, which folds into the clear, then a frosted sidebar over it with nothing
/// painted before it, then the page beside the sidebar.
fn first<S: Scene>(fixture: &mut SceneFrame<S>, ui: &mut Ui) {
    Panel::zstack()
        .id_salt("backdrop-first")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::canvas()
                .id_salt("backdrop-first-background")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect(), RgbaF32::srgb(0.12, 0.13, 0.16));
                });
            Panel::hstack()
                .id_salt("backdrop-first-row")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas()
                        .id_salt("backdrop-first-sidebar")
                        .size((Sizing::fixed(240.0), Sizing::FILL))
                        .show(ui, |ui| {
                            ui.add_shape(Shape::rect(), Look::Frosted.paint(SIGMA));
                        });
                    fixture.record(ui);
                });
        });
}

/// `integration/backdrop_frame/gpu/<scene>/<arm>`: one whole frame of each [`SceneCase`], wall
/// time, at 2×, waited for. Each case checks its settled frame's paint and blurs.
///
/// - `modal`: the scene at 4K under a frosted backdrop over the window; `cached` keeps every
///   frame still, `scrolling` scrolls the page under the backdrop, which captures every frame.
/// - `cards`: eight frosted cards in a 4 × 2 grid, 60 logical px apart, as a dashboard of glass
///   tiles sits: inside the 77.5 logical px a σ of 16 reaches, so each card's footprint takes in
///   its neighbors'.
/// - `cards_apart`: six frosted cards in a 3 × 2 grid, 100 logical px apart, past that reach.
/// - `first`: a frosted sidebar with nothing painted before it, which shows the clear, beside the
///   scrolling page.
/// - `stacked`: a glass popup over a frosted sidebar, which captures what the sidebar painted.
/// - `pair_near` and `pair_apart`: two frosted cards whose gap the second's reach covers or not.
/// - `many/under`: a strip under each row of twelve frosted cards flips color, staling more
///   backdrops than a partial frame holds, so the frame paints whole.
fn scenes<S: Scene>(group: &mut CaseGroup<'_>, backends: &Backends, backend: Backend) {
    for scene in SceneCase::<S>::ALL {
        group.bench_function(backend.case(scene.name), |bencher| {
            let gpu = backends.device(Timing::PassOnly);
            let target = gpu.target("palantir.bench.backdrop_scene", scene.surface);
            let mut host = OffscreenHost::builder(gpu.handles())
                .collect_gpu_stats(false)
                .swapchain_present()
                .build();
            let mut fixture = SceneFrame::<S>::default();
            let mut n = 0u32;
            let mut frame = || {
                n += 1;
                let mut app = |ui: &mut Ui| (scene.record)(&mut fixture, n, ui);
                let paint = host
                    .set_system_scale(FRAME_DPR)
                    .frame(target.as_target(), &mut app)
                    .paint();
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
