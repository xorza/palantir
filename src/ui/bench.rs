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
//! - **`resizing`**: rotates surface sizes so `available_q` busts the measure cache each frame.
//! - **`scrolling`**: shifts a `Panel::transform` so only the cascade walk sees change.
//! - **`alternating`**: a scroll step (full repaint) then a counter tick (small partial);
//!   measures what a full frame costs the partial after it.
//!
//! The surface is 2560 × 1440 at 2×, a 1280 × 720 window, and the fixture is taller, so the CPU
//! backend processes the whole tree while paint and the GPU see only the screen. The workload
//! lives in [`crate::internals::frame_fixture`], shared with the allocation gates in
//! `tests/alloc/gates/` and the showcase's `frame bench` page.

use crate::bench::fixture::{Backend, Backends, Kind, Scale};
use crate::gpu::bench_gpu::{BenchGpu, BenchTarget, Timing};
use crate::host::offscreen::OffscreenHost;
use crate::internals::frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};
use crate::internals::record_app::RecordApp;
use crate::internals::ui_harness::UiHarness;
use crate::internals::ui_harness::frontend_harness::FrontendHarness;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};
use std::slice;
use std::time::Instant;

/// The window clear both backends render over.
const WINDOW_CLEAR: RgbaF32 = RgbaF32::BLACK;
/// The surfaces the `resizing` scenario rotates through, proportioned against `BENCH_SURFACE`;
/// multiples of 16 keep each tile-aligned.
const RESIZE_POOL: [glam::UVec2; 4] = [
    glam::UVec2::new(2144, 1344),
    glam::UVec2::new(2560, 1440),
    glam::UVec2::new(2352, 1392),
    glam::UVec2::new(2768, 1488),
];

fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::PassOnly)
}

/// A host on the desktop's present strategy (`DirectAdaptive`, as the winit host
/// runs). The default would measure the screenshot path, which copies the
/// backbuffer out on skip and full frames.
fn bench_host(g: &BenchGpu) -> OffscreenHost {
    let mut host = g
        .offscreen_builder()
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
    Partial,
    Resizing,
    Scrolling,
}

impl Arm {
    /// Sorted by label, the order criterion lists them.
    const ALL: [Self; 5] = [
        Self::Alternating,
        Self::Cached,
        Self::Partial,
        Self::Resizing,
        Self::Scrolling,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Alternating => "alternating",
            Self::Cached => "cached",
            Self::Partial => "partial",
            Self::Resizing => "resizing",
            Self::Scrolling => "scrolling",
        }
    }

    /// The paint a settled frame `n` plans, cycling by `n`: pinned before the arm
    /// is measured, since a regression to another plan still prints a number.
    const fn paints(self) -> &'static [FramePaint] {
        match self {
            Self::Alternating => &[FramePaint::Full, FramePaint::Partial],
            Self::Cached => &[FramePaint::Skip],
            Self::Partial => &[FramePaint::Partial],
            Self::Resizing | Self::Scrolling => &[FramePaint::Full],
        }
    }

    /// The surfaces the arm renders at, frame `n` at `[n % len]`.
    const fn sizes(self) -> &'static [glam::UVec2] {
        match self {
            Self::Resizing => &RESIZE_POOL,
            Self::Alternating | Self::Cached | Self::Partial | Self::Scrolling => {
                slice::from_ref(&BENCH_SURFACE)
            }
        }
    }

    /// Set the fixture to frame `n`'s state.
    fn advance(self, state: &mut FrameFixture, n: u32) {
        match self {
            Self::Cached | Self::Resizing => {}
            Self::Partial => state.tick = state.tick.wrapping_add(1),
            Self::Scrolling => scroll(state),
            // A scroll step on even frames (full repaint), a counter tick on odd ones (small rect).
            Self::Alternating if n.is_multiple_of(2) => scroll(state),
            Self::Alternating => state.tick = state.tick.wrapping_add(1),
        }
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

/// Measure `arm` on `backend`: build its frame with `build` in the case, run it through
/// [`SETTLE_FRAMES`], pin one cycle of [`Arm::paints`], then time frames.
fn measure_arm<F>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    arm: Arm,
    backend: Backend,
    build: impl Fn() -> F,
) where
    F: FnMut(&mut FrameFixture, u32) -> FramePaint,
{
    group.bench_function(backend.case(arm.label()), |b| {
        let mut frame = build();
        let mut state = FrameFixture::default();
        let mut n = 0u32;
        let mut step = || {
            arm.advance(&mut state, n);
            let paint = frame(&mut state, n);
            n = n.wrapping_add(1);
            paint
        };
        for _ in 0..SETTLE_FRAMES {
            step();
        }
        for &want in arm.paints() {
            assert_eq!(
                step(),
                want,
                "the {}/{} frames plan another paint",
                backend.id(),
                arm.label()
            );
        }
        b.iter(&mut step);
    });
}

/// One target per surface the arm renders at.
fn targets(arm: Arm) -> Vec<BenchTarget> {
    arm.sizes()
        .iter()
        .enumerate()
        .map(|(i, size)| gpu().target(&format!("palantir.frame_bench.{}.{i}", arm.label()), *size))
        .collect()
}

fn gpu_frame(
    host: &mut OffscreenHost,
    target: &BenchTarget,
    system_scale: f32,
    state: &mut FrameFixture,
) -> FramePaint {
    let mut app = RecordApp::new(|ui| state.render(BENCH_SCALE, ui));
    host.frame(target.as_target(), system_scale, &mut app)
        .paint()
}

/// `integration/frame/<backend>/<scenario>`: one whole frame of the frame fixture.
pub(crate) fn integration(c: &mut Criterion, backends: Backends) {
    let mut group = Kind::Integration.group(c, "frame", Scale::Frame);
    for backend in backends.all() {
        for arm in Arm::ALL {
            match backend {
                Backend::Cpu => measure_arm(&mut group, arm, backend, || {
                    let mut h = CpuHarness::new();
                    let sizes = arm.sizes();
                    move |state: &mut FrameFixture, n: u32| {
                        if arm == Arm::Resizing {
                            h.frontend.harness.resize(sizes[n as usize % sizes.len()]);
                        }
                        h.frame(|ui| state.render(BENCH_SCALE, ui))
                    }
                }),
                Backend::Gpu => measure_arm(&mut group, arm, backend, || {
                    let g = gpu();
                    let targets = targets(arm);
                    let mut host = bench_host(g);
                    move |state: &mut FrameFixture, n: u32| {
                        let target = &targets[n as usize % targets.len()];
                        let paint = gpu_frame(&mut host, target, BENCH_DPR, state);
                        g.wait();
                        paint
                    }
                }),
            }
        }
    }
    group.finish();
}
