//! The frame: palantir's whole pipeline over the frame fixture, on each backend.
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
//! - **`partial`**: one fixture counter changes per frame, so damage is one small `Partial` rect.
//! - **`resizing`**: rotates surface sizes so the offer busts the measure cache each frame.
//! - **`scrolling`**: shifts a `Panel::transform` so only the cascade walk sees change.
//! - **`alternating`**: a scroll step (full repaint) then a counter tick (small partial);
//!   measures what a full frame costs the partial after it.
//! - **`hovering`**: the pointer moves from the request card's checkbox to its submit button and
//!   back, one move a frame: input routing, hover state and its small partial repaint.
//! - **`typing`**: the request card's name field holds the focus and takes a character, then a
//!   backspace, one a frame: an edit's reshaping, caret and small partial repaint.
//!
//! The surface is 2560 × 1440 at 2×, a 1280 × 720 window, and the fixture is taller, so the CPU
//! backend processes the whole tree while paint and the GPU see only the screen. The workload
//! lives in [`crate::internals::frame_fixture`], shared with the allocation gates in
//! `tests/alloc/gates/` and the showcase's `frame bench` page.

use crate::bench::backends::{Backend, Backends};
use crate::bench::case_group::CaseGroup;
use crate::bench::fixture;
use crate::bench::kind::{Kind, Scale};
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::host::offscreen::OffscreenHost;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::internals::frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};
use crate::internals::record_app::RecordApp;
use crate::internals::ui_harness::UiHarness;
use crate::internals::ui_harness::frontend_harness::FrontendHarness;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::window::window_token::WindowToken;
use criterion::Criterion;
use glam::UVec2;
use std::slice;
use std::time::Instant;

/// The window clear both backends render over.
const WINDOW_CLEAR: RgbaF32 = RgbaF32::BLACK;
/// The surfaces the `resizing` scenario rotates through, proportioned against `BENCH_SURFACE`;
/// multiples of 16 keep each tile-aligned.
const RESIZE_POOL: [UVec2; 4] = [
    UVec2::new(2144, 1344),
    UVec2::new(2560, 1440),
    UVec2::new(2352, 1392),
    UVec2::new(2768, 1488),
];

fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::PassOnly)
}

/// A host on the desktop's present strategy (`DirectAdaptive`, as the winit host
/// runs). The default would measure the screenshot path, which copies the
/// backbuffer out on skip and full frames.
fn bench_host(g: &BenchGpu) -> OffscreenHost {
    let mut host = OffscreenHost::builder(g.handles())
        .collect_gpu_stats(false)
        .retained_target(true)
        .build();
    host.ui().theme_mut().window_clear = WINDOW_CLEAR;
    host
}

/// Deviceless CPU-pipeline harness: a bare `Ui` plus a standalone `Frontend`,
/// stopping before any GPU submit. Time comes from a real `Instant` as in
/// `WindowDriver::cpu_frame`; a frozen clock could classify frames `PaintOnly` and
/// skip the record closure.
#[derive(Debug)]
struct CpuHarness {
    frontend: FrontendHarness,
    start: Instant,
}

impl CpuHarness {
    fn new() -> Self {
        let mut harness = UiHarness::with_text(BENCH_SURFACE).scale(BENCH_DPR);
        harness.ui.theme_mut().window_clear = WINDOW_CLEAR;
        Self {
            frontend: FrontendHarness::new(harness),
            start: Instant::now(),
        }
    }

    /// Drive one full CPU frame and ack the present, so `cached` settles into `Skip`.
    fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FramePaint {
        self.frontend.harness.at(self.start.elapsed());
        self.frontend.frame(record).paint()
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
    Typing,
}

impl Arm {
    /// Sorted by label, the order criterion lists them.
    const ALL: [Self; 7] = [
        Self::Alternating,
        Self::Cached,
        Self::Hovering,
        Self::Partial,
        Self::Resizing,
        Self::Scrolling,
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
            | Self::Typing => slice::from_ref(&BENCH_SURFACE),
        }
    }

    /// Set the fixture to frame `n`'s state.
    fn advance(self, state: &mut FrameFixture, n: u32) {
        match self {
            Self::Cached | Self::Hovering | Self::Resizing | Self::Typing => {}
            Self::Partial => state.tick = state.tick.wrapping_add(1),
            Self::Scrolling => scroll(state),
            // A scroll step on even frames (full repaint), a counter tick on odd ones (small rect).
            Self::Alternating if n.is_multiple_of(2) => scroll(state),
            Self::Alternating => state.tick = state.tick.wrapping_add(1),
        }
    }

    /// The input frame `n` sends `ui` before it records, after the arm puts the focus where it
    /// types: none until the last frame resolved the widgets the arm aims at.
    fn input(self, state: &FrameFixture, ui: &mut Ui, n: u32) -> Option<InputEvent<'static>> {
        let probes = state.probes;
        match self {
            Self::Hovering => {
                let id = if n.is_multiple_of(2) {
                    probes.enabled
                } else {
                    probes.submit
                }?;
                let rect = ui.response_for(id).rect?;
                Some(InputEvent::PointerMoved(rect.center()))
            }
            Self::Typing => {
                let edit = probes.name_edit?;
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
            Self::Alternating | Self::Cached | Self::Partial | Self::Resizing | Self::Scrolling => {
                None
            }
        }
    }

    /// One target per surface the arm renders at.
    fn targets(self) -> Vec<OwnedTarget> {
        self.sizes()
            .iter()
            .enumerate()
            .map(|(i, size)| {
                gpu().target(&format!("palantir.frame_bench.{}.{i}", self.label()), *size)
            })
            .collect()
    }

    /// Measure this arm on `backend`: build its frame with `build` in the case, run it through
    /// [`SETTLE_FRAMES`], pin one cycle of [`Self::paints`], then time frames.
    fn measure<F>(self, group: &mut CaseGroup<'_>, backend: Backend, build: impl Fn() -> F)
    where
        F: FnMut(&mut FrameFixture, u32) -> FramePaint,
    {
        group.bench_function(backend.case(self.label()), |b| {
            let mut frame = build();
            let mut state = FrameFixture::default();
            let mut n = 0u32;
            // A frame's paint, and whether the typing arm's key reached the field: a character
            // after an even frame, none after an odd.
            let mut step = || {
                self.advance(&mut state, n);
                let paint = frame(&mut state, n);
                let typed = self != Self::Typing
                    || state.name() == if n.is_multiple_of(2) { "x" } else { "" };
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
/// the frame fixture (the `scrolling` arm), from its in-pass timestamps: the quads, the shadows
/// and the text, each pipeline's share of [`main_pass`]. Only where the adapter writes in-pass
/// timestamps; a tiler splits its pass at each, so these cases do not sum to `main_pass`.
fn main_pass_batch(c: &mut Criterion, backends: Backends) {
    let Some(backend) = backends.batch_timed_gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "main_pass_batch", Scale::Frame);
    for kind in [BatchKind::Quads, BatchKind::Shadows, BatchKind::Text] {
        group.bench_function(backend.case(kind.label()), |b| {
            let g = BenchGpu::shared(Timing::Instrumented);
            let target = g.target("palantir.main_pass_batch_bench", BENCH_SURFACE);
            let mut host = OffscreenHost::builder(g.handles())
                .collect_gpu_stats(true)
                .retained_target(true)
                .build();
            host.ui().theme_mut().window_clear = WINDOW_CLEAR;
            let mut state = FrameFixture::default();
            let mut n = 0u32;
            let mut frame = || {
                Arm::Scrolling.advance(&mut state, n);
                n = n.wrapping_add(1);
                let paint = gpu_frame(&mut host, &target, &mut state);
                g.wait();
                g.poll();
                (paint, host.gpu_pass_stats().last_kind(kind))
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

/// Shift the fixture's transform, so only the cascade walk sees change.
fn scroll(state: &mut FrameFixture) {
    state.scroll_offset.x = (state.scroll_offset.x + 1.5) % 256.0;
    state.scroll_offset.y = (state.scroll_offset.y + 0.7) % 256.0;
}

/// Frames each arm runs before its paint is pinned: even, so the alternating arm
/// starts its check on a scroll step.
const SETTLE_FRAMES: u32 = 4;

/// One frame of the fixture on `target`, at the bench's device pixel ratio.
fn gpu_frame(
    host: &mut OffscreenHost,
    target: &OwnedTarget,
    state: &mut FrameFixture,
) -> FramePaint {
    let mut app = RecordApp::new(|ui| state.render(BENCH_SCALE, ui));
    host.frame(target.as_target(), BENCH_DPR, &mut app).paint()
}

/// `integration/frame/<backend>/<scenario>`: one whole frame of the frame fixture.
pub(crate) fn integration(c: &mut Criterion, backends: Backends) {
    let mut group = Kind::Integration.group(c, "frame", Scale::Frame);
    for backend in backends.all() {
        for arm in Arm::ALL {
            match backend {
                Backend::Cpu => arm.measure(&mut group, backend, || {
                    let mut h = CpuHarness::new();
                    let sizes = arm.sizes();
                    move |state: &mut FrameFixture, n: u32| {
                        if arm == Arm::Resizing {
                            h.frontend.harness.resize(sizes[n as usize % sizes.len()]);
                        }
                        let harness = &mut h.frontend.harness;
                        if let Some(event) = arm.input(state, &mut harness.ui, n) {
                            harness.on_input(event);
                            release(event, |up| {
                                harness.on_input(up);
                            });
                        }
                        h.frame(|ui| state.render(BENCH_SCALE, ui))
                    }
                }),
                Backend::Gpu => arm.measure(&mut group, backend, || {
                    let g = gpu();
                    let targets = arm.targets();
                    let mut host = bench_host(g);
                    move |state: &mut FrameFixture, n: u32| {
                        let target = &targets[n as usize % targets.len()];
                        if let Some(event) = arm.input(state, host.ui(), n) {
                            host.on_input(event);
                            release(event, |up| {
                                host.on_input(up);
                            });
                        }
                        let paint = gpu_frame(&mut host, target, state);
                        g.wait();
                        paint
                    }
                }),
            }
        }
    }
    group.finish();
}

/// Two stages of the frame alone over the frame fixture: record, and the main pass.
pub(crate) fn atomic(c: &mut Criterion, backends: Backends) {
    record(c);
    main_pass(c, backends);
    main_pass_batch(c, backends);
}

/// `atomic/record/cpu/frame`: one record pass of the frame fixture alone, its hashes finalized:
/// the widgets' code, node and shape recording and lowering, without layout or what follows.
fn record(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "record", Scale::Sample);
    group.bench_function(Backend::Cpu.case("frame"), |b| {
        let mut state = FrameFixture::default();
        let mut h = UiHarness::with_text(BENCH_SURFACE).scale(BENCH_DPR);
        h.frame(|ui| state.render(BENCH_SCALE, ui));
        let mut app = RecordApp::new(|ui: &mut Ui| state.render(BENCH_SCALE, ui));
        let mut pass = || h.ui.record_alone(&mut h.engines, WindowToken(0), &mut app);
        pass();
        b.iter(&mut pass);
    });
    group.finish();
}

/// `atomic/main_pass/gpu/<arm>`: the device time of the main pass alone over the frame fixture,
/// read back from its timestamps: `scrolling`, a whole repaint each frame, and `partial`, the
/// footer counter's small rect. What the GPU pays to paint the frame, without the CPU half
/// [`integration`] adds. On Metal the pass time is the frame's wall time from submit to done
/// ([`GpuTimings`](crate::gpu::frame::gpu_timings::GpuTimings)), so it also holds the frame's
/// uploads, copies and the wake-up's latency.
fn main_pass(c: &mut Criterion, backends: Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "main_pass", Scale::Frame);
    for arm in [Arm::Partial, Arm::Scrolling] {
        group.bench_function(backend.case(arm.label()), |b| {
            let g = gpu();
            let target = g.target("palantir.main_pass_bench", BENCH_SURFACE);
            let mut host = OffscreenHost::builder(g.handles())
                .collect_gpu_stats(true)
                .retained_target(true)
                .build();
            host.ui().theme_mut().window_clear = WINDOW_CLEAR;
            let mut state = FrameFixture::default();
            let mut n = 0u32;
            let mut frame = || {
                arm.advance(&mut state, n);
                n = n.wrapping_add(1);
                let paint = gpu_frame(&mut host, &target, &mut state);
                g.wait();
                g.poll();
                (paint, host.gpu_pass_stats().last_pass())
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
