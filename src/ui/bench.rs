//! The frame: palantir's whole pipeline over the scene, on each backend.
//!
//! - **`cpu`**: the CPU pipeline on a bare `Ui` and standalone `Frontend` with no wgpu device,
//!   from record through encode and compose. Going through the offscreen driver plus a poll
//!   charges every iteration driver time (~20% on `cached`, ~50% on `resizing`) that swamps the
//!   cost measured, and a `'/cpu/'` run opens no device, so a `perf` capture is clean.
//! - **`gpu`**: `OffscreenHost::frame` against an offscreen texture, waited for, on the desktop's
//!   present strategy (see `bench_host`).
//!
//! Scenarios, on both backends:
//!
//! - **`cached`**: fixed viewport, measure-cache hits, damage `Skip`.
//! - **`partial`**: the counter over the scene changes per frame, so damage is one small `Partial`
//!   rect.
//! - **`resizing`**: rotates surface sizes so the offer busts the measure cache each frame.
//! - **`scrolling`**: shifts a `Panel::transform` so only the cascade walk sees change.
//! - **`alternating`**: a scroll step (full repaint) then a counter tick (small partial);
//!   measures what a full frame costs the partial after it.
//! - **`hovering`**: the pointer moves between the scene's two hover targets, one move a frame:
//!   input routing, hover state and its small partial repaint.
//! - **`typing`**: the scene's field holds the focus and takes a character, then a backspace, one
//!   a frame: an edit's reshaping, caret and small partial repaint.
//! - **`theming`**: a theme transition between the dark stock look and a roomier light one, every
//!   frame at a new phase: a theme blend, a record under it and a whole repaint, with each font
//!   size at a new value, so text reshapes and rasterizes anew. The worst frame a transition
//!   costs; [`THEMING`] runs so long that a measured frame never lands.
//!
//! The surface is 2560 × 1440 at 2×, a 1280 × 720 window. The scene comes from the caller
//! ([`Scene`]). Motion, transparency and power are pinned full, so no default that moves can
//! change a number.
//! The bench theme still snaps its state transitions, as [`bench_theme`] says why.

use crate::theme::color_scheme::ColorScheme;

use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::bench::backends::{Backend, Backends};
use crate::bench::case_group::CaseGroup;
use crate::bench::fixture;
use crate::bench::kind::{Kind, Scale};
use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE, Scene, SceneFrame};
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::host::offscreen::{OffscreenHost, OffscreenTime};
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::theme::Theme;
use crate::theme::metrics::{Metrics, SpaceSteps, TextSteps};
use crate::theme::palette::Palette;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::ui::ui_preferences::UiPreferences;
use criterion::Criterion;
use glam::UVec2;
use std::rc::Rc;
use std::slice;
use std::time::{Duration, Instant};

/// The window clear both backends render over.
const WINDOW_CLEAR: RgbaF32 = RgbaF32::BLACK;

/// The stock look over [`WINDOW_CLEAR`], its own arrival and its state transitions snapped: an arm
/// that changes a look each frame measures the repaint the change plans, where a transition starts each change
/// from rest and paints nothing in its first frame, and the hovering arm, which moves every
/// frame, would plan none.
fn bench_theme() -> Theme {
    Theme {
        window_bg: WINDOW_CLEAR,
        transition: AnimationSpec::SNAP,
        ..Theme::from_parts(
            &Palette::DARK,
            &Metrics {
                transition: AnimationSpec::SNAP,
                ..Metrics::DEFAULT
            },
        )
    }
}

/// Motion, transparency and power whole, whatever the host's settings say.
fn force_full_effects(ui: &mut Ui) {
    ui.set_preferences(UiPreferences {
        motion: Preference::Fixed(Reduction::Full),
        transparency: Preference::Fixed(Reduction::Full),
        power: Preference::Fixed(Reduction::Full),
        ..ui.preferences()
    });
}

/// Frame `n`'s look: the scene builds its theme on the first frame, so the bench's goes on the
/// second, at once.
pub(crate) fn settle_look(ui: &mut Ui, n: u32) {
    if n == 1 {
        ui.set_theme(ColorScheme::Dark, bench_theme()).unwrap();
    }
}

/// The theming arm's transition: linear over a minute, so its phase moves on every frame and a
/// landing, which would show a still frame, comes once in 3,600.
const THEMING: AnimationSpec = AnimationSpec::duration(Duration::from_secs(60), Easing::Linear);

/// The theming arm's frame: a 60 Hz step of a clock the arm moves itself, so each measured frame
/// moves the transition by the same phase however long the frame took, and two runs blend the
/// same themes.
const THEMING_FRAME: Duration = Duration::from_nanos(16_666_667);

/// The theming arm's other end: the light palette, text 4 px larger and spacing a third wider,
/// as a density preset moves both.
fn roomy_theme() -> Theme {
    let d = Metrics::DEFAULT;
    Theme {
        window_bg: WINDOW_CLEAR,
        ..Theme::from_parts(
            &Palette::LIGHT,
            &Metrics {
                text: TextSteps {
                    caption: d.text.caption + 4.0,
                    body: d.text.body + 4.0,
                    title: d.text.title + 4.0,
                },
                space: SpaceSteps {
                    xxs: d.space.xxs * 4.0 / 3.0,
                    xs: d.space.xs * 4.0 / 3.0,
                    sm: d.space.sm * 4.0 / 3.0,
                    md: d.space.md * 4.0 / 3.0,
                    lg: d.space.lg * 4.0 / 3.0,
                    xl: d.space.xl * 4.0 / 3.0,
                },
                transition: AnimationSpec::SNAP,
                ..d
            },
        )
    }
}

/// The surfaces the `resizing` scenario rotates through, proportioned against `BENCH_SURFACE`;
/// multiples of 16 keep each tile-aligned.
const RESIZE_POOL: [UVec2; 4] = [
    UVec2::new(2144, 1344),
    UVec2::new(2560, 1440),
    UVec2::new(2352, 1392),
    UVec2::new(2768, 1488),
];

/// The device timing every GPU case here opens.
pub(crate) const TIMING: Timing = Timing::PassOnly;

/// A host on the desktop's present strategy (`DirectAdaptive`, as the winit host
/// runs). The default would measure the screenshot path, which copies the
/// backbuffer out on skip and full frames, and a retained target paints a
/// partial frame where the desktop cannot.
pub(crate) fn bench_host(g: &BenchGpu, time: OffscreenTime) -> OffscreenHost {
    let mut host = OffscreenHost::builder(g.handles())
        .collect_gpu_stats(false)
        .swapchain_present()
        .time(time)
        .build();
    force_full_effects(host.ui_mut());
    host
}

/// Deviceless CPU-pipeline harness: a bare `Ui` plus a standalone `Frontend`,
/// stopping before any GPU submit. Time comes from [`CpuClock`]: a real `Instant` unless the arm
/// steps its own, since a frozen clock could classify frames `PaintOnly` and skip the record
/// closure.
#[derive(Debug)]
struct CpuHarness {
    h: Harness,
    clock: CpuClock,
}

/// The time a [`CpuHarness`] frame carries.
#[derive(Debug)]
enum CpuClock {
    /// Since this `Instant`.
    Realtime(Instant),
    /// [`THEMING_FRAME`] a frame, this many frames run.
    Stepped(u32),
}

impl CpuHarness {
    fn new(time: OffscreenTime) -> Self {
        let mut h = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .depth(Depth::Compose)
            .build();
        force_full_effects(h.ui_mut());
        Self {
            h,
            clock: match time {
                OffscreenTime::Realtime => CpuClock::Realtime(Instant::now()),
                OffscreenTime::Manual => CpuClock::Stepped(0),
            },
        }
    }

    /// Drive one full CPU frame and ack the present, so `cached` settles into `Skip`.
    fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FramePaint {
        let time = match &mut self.clock {
            CpuClock::Realtime(start) => start.elapsed(),
            CpuClock::Stepped(frames) => {
                *frames += 1;
                THEMING_FRAME * *frames
            }
        };
        self.h.at(time);
        self.h.frame(record).paint()
    }
}

/// What changes between two frames of one arm. One list for both halves, so a
/// CPU arm and its GPU twin always measure the same frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Alternating,
    Cached,
    Hovering,
    Partial,
    Resizing,
    Scrolling,
    Theming,
    Typing,
}

impl Arm {
    /// Sorted by label, the order criterion lists them.
    const ALL: [Self; 8] = [
        Self::Alternating,
        Self::Cached,
        Self::Hovering,
        Self::Partial,
        Self::Resizing,
        Self::Scrolling,
        Self::Theming,
        Self::Typing,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Alternating => "alternating",
            Self::Cached => "cached",
            Self::Hovering => "hovering",
            Self::Partial => "partial",
            Self::Resizing => "resizing",
            Self::Scrolling => "scrolling",
            Self::Theming => "theming",
            Self::Typing => "typing",
        }
    }

    /// The paint a settled frame `n` plans, cycling by `n`: pinned before the arm
    /// is measured, since a regression to another plan still prints a number.
    const fn paints(self) -> &'static [FramePaint] {
        match self {
            Self::Alternating => &[FramePaint::Full, FramePaint::Partial],
            Self::Cached => &[FramePaint::Skip],
            Self::Hovering | Self::Partial | Self::Typing => &[FramePaint::Partial],
            Self::Resizing | Self::Scrolling => &[FramePaint::Full],
            // Both themes clear the window alike, so damage covers what the blend moves, not
            // the whole window.
            Self::Theming => &[FramePaint::Partial],
        }
    }

    /// The surfaces the arm renders at, frame `n` at `[n % len]`.
    const fn sizes(self) -> &'static [UVec2] {
        match self {
            Self::Resizing => &RESIZE_POOL,
            Self::Alternating
            | Self::Cached
            | Self::Hovering
            | Self::Partial
            | Self::Scrolling
            | Self::Theming
            | Self::Typing => slice::from_ref(&BENCH_SURFACE),
        }
    }

    /// Set the frame to frame `n`'s state.
    fn advance<S>(self, state: &mut SceneFrame<S>, n: u32) {
        match self {
            Self::Cached | Self::Hovering | Self::Resizing | Self::Theming | Self::Typing => {}
            Self::Partial => state.tick = state.tick.wrapping_add(1),
            Self::Scrolling => scroll(state),
            // A scroll step on even frames (full repaint), a counter tick on odd ones (small rect).
            Self::Alternating if n.is_multiple_of(2) => scroll(state),
            Self::Alternating => state.tick = state.tick.wrapping_add(1),
        }
    }

    /// The input frame `n` sends `ui` before it records, after the arm puts the focus where it
    /// types: none until the last frame resolved the widgets the arm aims at.
    fn input<S: Scene>(
        self,
        state: &SceneFrame<S>,
        ui: &mut Ui,
        n: u32,
    ) -> Option<InputEvent<'static>> {
        match self {
            Self::Hovering => {
                let id = state.scene.hover_targets()[n as usize % 2];
                let rect = ui.response_for(id).rect?;
                Some(InputEvent::PointerMoved(rect.center()))
            }
            Self::Typing => {
                let edit = state.scene.field();
                if ui.focus() != Some(edit) {
                    ui.set_focus(edit);
                }
                let key = if n.is_multiple_of(2) {
                    Key::Char('x')
                } else {
                    Key::Backspace
                };
                Some(InputEvent::key_down(key))
            }
            Self::Alternating
            | Self::Cached
            | Self::Partial
            | Self::Resizing
            | Self::Scrolling
            | Self::Theming => None,
        }
    }

    /// Ready `ui` for frame `n`: the theming arm starts a transition to the theme it does not
    /// show whenever none runs, once the bench's look is on and after a rare landing.
    fn prepare(self, ui: &mut Ui, n: u32) {
        settle_look(ui, n);
        if self != Self::Theming
            || n < 2
            || !Rc::ptr_eq(ui.theme(), ui.scheme_theme(ui.color_scheme()))
        {
            return;
        }
        let roomy = ui.theme().text.font_size == Metrics::DEFAULT.text.body;
        ui.set_theme(
            ColorScheme::Dark,
            Theme {
                transition: THEMING,
                ..if roomy { roomy_theme() } else { bench_theme() }
            },
        )
        .unwrap();
    }

    /// The clock its frames read: the wall clock, or one the arm steps itself.
    const fn clock(self) -> OffscreenTime {
        match self {
            Self::Theming => OffscreenTime::Manual,
            Self::Alternating
            | Self::Cached
            | Self::Hovering
            | Self::Partial
            | Self::Resizing
            | Self::Scrolling
            | Self::Typing => OffscreenTime::Realtime,
        }
    }

    /// One target per surface the arm renders at.
    fn targets(self, gpu: &BenchGpu) -> Vec<OwnedTarget> {
        self.sizes()
            .iter()
            .enumerate()
            .map(|(i, size)| {
                gpu.target(&format!("palantir.frame_bench.{}.{i}", self.label()), *size)
            })
            .collect()
    }

    /// Measure this arm on `backend`: build its frame with `build` in the case, run it through
    /// [`SETTLE_FRAMES`], pin one cycle of [`Self::paints`], then time frames.
    fn measure<S, F>(self, group: &mut CaseGroup<'_>, backend: Backend, build: impl Fn() -> F)
    where
        S: Scene,
        F: FnMut(&mut SceneFrame<S>, u32) -> FramePaint,
    {
        group.bench_function(backend.case(self.label()), |b| {
            let mut frame = build();
            let mut state = SceneFrame::<S>::default();
            let mut n = 0u32;
            // A frame's paint, and whether the typing arm's key reached the field: a character
            // after an even frame, none after an odd.
            let mut step = || {
                self.advance(&mut state, n);
                let paint = frame(&mut state, n);
                let typed = self != Self::Typing
                    || state.scene.field_len() == usize::from(n.is_multiple_of(2));
                n = n.wrapping_add(1);
                (paint, typed)
            };
            for _ in 0..SETTLE_FRAMES {
                step();
            }
            for &want in self.paints() {
                let (paint, typed) = step();
                assert_eq!(
                    paint,
                    want,
                    "the {}/{} frames plan another paint",
                    backend.id(),
                    self.label()
                );
                assert!(typed, "{}/typing: the key misses the field", backend.id());
            }
            b.iter(&mut step);
        });
    }
}

/// `atomic/main_pass_batch/gpu/<kind>`: the device time of one batch kind of a whole repaint of
/// the scene (the `scrolling` arm), from its in-pass timestamps: the quads and the text, each
/// pipeline's share of [`main_pass`]. No shadow case: the page paints no shadow, and a case with
/// no batch reads no timestamp. Only where the adapter writes in-pass
/// timestamps; a tiler splits its pass at each, so these cases do not sum to `main_pass`.
fn main_pass_batch<S: Scene>(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.batch_timed_gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "main_pass_batch", Scale::Frame);
    for kind in [BatchKind::Quads, BatchKind::Text] {
        group.bench_function(backend.case(kind.label()), |b| {
            let g = backends.device(Timing::Instrumented);
            let target = g.target("palantir.main_pass_batch_bench", BENCH_SURFACE);
            let mut host = OffscreenHost::builder(g.handles())
                .collect_gpu_stats(true)
                .swapchain_present()
                .build();
            force_full_effects(host.ui_mut());
            let mut state = SceneFrame::<S>::default();
            let mut n = 0u32;
            let mut frame = || {
                settle_look(host.ui_mut(), n);
                Arm::Scrolling.advance(&mut state, n);
                n = n.wrapping_add(1);
                let paint = gpu_frame(&mut host, &target, &mut state);
                g.wait();
                g.poll();
                (paint, host.ui().gpu_pass_stats().last_kind(kind))
            };
            for _ in 0..SETTLE_FRAMES {
                frame();
            }
            assert_eq!(
                frame().0,
                FramePaint::Full,
                "main_pass_batch/{}",
                kind.label()
            );
            let mut time = || frame().1;
            fixture::settle_timestamps(&mut time);
            b.iter_custom(|iters| fixture::device_time(iters, &mut time));
        });
    }
    group.finish();
}

/// Send through `send` the release of a key `event` presses, so the next press is not a repeat.
fn release(event: InputEvent<'static>, send: impl FnOnce(InputEvent<'static>)) {
    if let InputEvent::KeyDown { key, .. } = event {
        send(InputEvent::KeyUp { key });
    }
}

/// Shift the scene's transform, so only the cascade walk sees change.
fn scroll<S>(state: &mut SceneFrame<S>) {
    state.scroll.x = (state.scroll.x + 1.5) % 256.0;
    state.scroll.y = (state.scroll.y + 0.7) % 256.0;
}

/// Frames each arm runs before its paint is pinned: even, so the alternating arm
/// starts its check on a scroll step.
const SETTLE_FRAMES: u32 = 4;

/// One frame of the scene on `target`, at the bench's device pixel ratio.
fn gpu_frame<S: Scene>(
    host: &mut OffscreenHost,
    target: &OwnedTarget,
    state: &mut SceneFrame<S>,
) -> FramePaint {
    let mut app = |ui: &mut Ui| state.record(ui);
    host.set_system_scale(BENCH_DPR)
        .frame(target.as_target(), &mut app)
        .paint()
}

/// One arm's frames on the device: its host and the targets it renders at.
#[derive(Debug)]
struct GpuArm {
    arm: Arm,
    host: OffscreenHost,
    targets: Vec<OwnedTarget>,
}

impl GpuArm {
    fn new(arm: Arm, g: &BenchGpu) -> Self {
        Self {
            arm,
            host: bench_host(g, arm.clock()),
            targets: arm.targets(g),
        }
    }

    /// Frame `n` of the arm over `state`, waited for.
    fn frame<S: Scene>(&mut self, g: &BenchGpu, state: &mut SceneFrame<S>, n: u32) -> FramePaint {
        let Self { arm, host, targets } = self;
        let target = &targets[n as usize % targets.len()];
        if arm.clock() == OffscreenTime::Manual {
            host.advance(THEMING_FRAME);
        }
        arm.prepare(host.ui_mut(), n);
        if let Some(event) = arm.input(state, host.ui_mut(), n) {
            host.on_input(event);
            release(event, |up| {
                host.on_input(up);
            });
        }
        let paint = gpu_frame(host, target, state);
        g.wait();
        paint
    }
}

/// Run each arm of `integration/frame` on the device for `frames` frames, a host of its own each,
/// and hand `visit` the arm's label and its host after every frame.
pub(crate) fn drive_arms<S: Scene>(
    g: &BenchGpu,
    frames: u32,
    mut visit: impl FnMut(&str, &OffscreenHost),
) {
    for arm in Arm::ALL {
        let mut run = GpuArm::new(arm, g);
        let mut state = SceneFrame::<S>::default();
        for n in 0..frames {
            arm.advance(&mut state, n);
            run.frame(g, &mut state, n);
            visit(arm.label(), &run.host);
        }
    }
}

/// `integration/frame/<backend>/<scenario>`: one whole frame of the scene.
pub(crate) fn integration<S: Scene>(c: &mut Criterion, backends: &Backends) {
    let mut group = Kind::Integration.group(c, "frame", Scale::Frame);
    for backend in backends.all() {
        for arm in Arm::ALL {
            match backend {
                Backend::Cpu => arm.measure(&mut group, backend, || {
                    let mut h = CpuHarness::new(arm.clock());
                    let sizes = arm.sizes();
                    move |state: &mut SceneFrame<S>, n: u32| {
                        if arm == Arm::Resizing {
                            h.h.resize(sizes[n as usize % sizes.len()]);
                        }
                        let harness = &mut h.h;
                        arm.prepare(harness.ui_mut(), n);
                        if let Some(event) = arm.input(state, harness.ui_mut(), n) {
                            harness.on_input(event);
                            release(event, |up| {
                                harness.on_input(up);
                            });
                        }
                        h.frame(|ui| state.record(ui))
                    }
                }),
                Backend::Gpu => arm.measure(&mut group, backend, || {
                    let g = backends.device(TIMING);
                    let mut run = GpuArm::new(arm, g);
                    move |state: &mut SceneFrame<S>, n: u32| run.frame(g, state, n)
                }),
            }
        }
    }
    group.finish();
}

/// The frame's stages alone over the scene: record, the main pass, and the main pass by batch
/// kind.
pub(crate) fn atomic<S: Scene>(c: &mut Criterion, backends: &Backends) {
    record::<S>(c);
    main_pass::<S>(c, backends);
    main_pass_batch::<S>(c, backends);
}

/// `atomic/record/cpu/frame`: one record pass of the scene alone, its hashes finalized:
/// the widgets' code, node and shape recording and lowering, without layout or what follows.
fn record<S: Scene>(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "record", Scale::Sample);
    group.bench_function(Backend::Cpu.case("frame"), |b| {
        let mut state = SceneFrame::<S>::default();
        let mut h = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .build();
        h.frame(|ui| state.record(ui));
        let mut app = |ui: &mut Ui| state.record(ui);
        let mut pass = || {
            h.frames_mut().record_alone(&mut app);
        };
        pass();
        b.iter(&mut pass);
    });
    group.finish();
}

/// `atomic/main_pass/gpu/<arm>`: the device time of the main pass alone over the scene,
/// read back from its timestamps: `scrolling`, a whole repaint each frame, and `partial`, the
/// counter's small rect. What the GPU pays to paint the frame, without the CPU half
/// [`integration`] adds. On Metal the pass time is the frame's wall time from submit to done
/// ([`GpuTimings`](crate::gpu::frame::gpu_timings::GpuTimings)), so it also holds the frame's
/// uploads, copies and the wake-up's latency.
fn main_pass<S: Scene>(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "main_pass", Scale::Frame);
    for arm in [Arm::Partial, Arm::Scrolling] {
        group.bench_function(backend.case(arm.label()), |b| {
            let g = backends.device(TIMING);
            let target = g.target("palantir.main_pass_bench", BENCH_SURFACE);
            let mut host = OffscreenHost::builder(g.handles())
                .collect_gpu_stats(true)
                .swapchain_present()
                .build();
            force_full_effects(host.ui_mut());
            let mut state = SceneFrame::<S>::default();
            let mut n = 0u32;
            let mut frame = || {
                settle_look(host.ui_mut(), n);
                arm.advance(&mut state, n);
                n = n.wrapping_add(1);
                let paint = gpu_frame(&mut host, &target, &mut state);
                g.wait();
                g.poll();
                (paint, host.ui().gpu_pass_stats().last_pass())
            };
            for _ in 0..SETTLE_FRAMES {
                frame();
            }
            assert_eq!(frame().0, arm.paints()[0], "main_pass/{}", arm.label());
            let mut time = || frame().1;
            fixture::settle_timestamps(&mut time);
            b.iter_custom(|iters| fixture::device_time(iters, &mut time));
        });
    }
    group.finish();
}
