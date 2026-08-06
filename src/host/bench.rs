//! Per-frame allocation gates for the frame pipeline, driven by `dhat`.
//!
//! One bench — [`alloc`] — of two steps, each warming up and then
//! counting what a batch of steady-state frames allocates:
//!
//! | step | measures | limit |
//! |---|---|---|
//! | `record-only` | palantir's CPU pipeline at full scale: record → measure → arrange → cascade → damage. No GPU, and no paint — `Ui::frame` stops before the frontend | strict zero |
//! | `record + render` | a whole frame through `OffscreenHost::frame_offscreen`, encode and compose included, down to the wgpu submission | the driver floor |
//!
//! Two, because those are the two questions this binary can answer that
//! nothing else can: whether *our* per-frame code allocates at full
//! scale, and whether the driver floor beneath it has drifted.
//!
//! **Everything finer-grained lives in `tests/alloc/`** — a per-frame
//! audit with backtrace capture over ~20 fixtures. It covers the churn
//! shapes a whole-window delta can only report in aggregate (width
//! drag, changing text, rows entering and leaving), and the encode +
//! compose paths step 1 doesn't reach, in `fixtures/renderer.rs`. That
//! suite runs in `cargo test` in under a second with no allocator tax,
//! so it is the right place for anything wanting attribution rather
//! than a gate. The two steps here earn their place by being what it
//! cannot reach: it is GPU-less, and it audits small scenes rather than
//! the full tree.
//!
//! This is its own target only because `dhat::Alloc` must be the
//! process-wide global allocator, which would tax every criterion timing
//! sharing the binary 10-30x. For the same reason these numbers are
//! allocation counts, never times.

use crate::app::App;
use crate::frame_fixture::{BENCH_SCALE, FrameFixture};
use crate::host::bench_gpu::{BenchGpu, Timing};
use crate::primitives::color::Color;
use crate::ui::Ui;
use crate::ui::bench::{CACHED_SIZE, SCALE};
use crate::ui::harness::UiHarness;
use glam::UVec2;
use std::hint::black_box;

// 256 measure frames so an intermittent grow-on-Nth-frame allocation
// (Vec doubling, HashMap rehash) isn't lost between two snapshots.
const MEASURE_FRAMES: usize = 256;

/// `--dump` swaps the counting-only profiler for the heap profiler that
/// writes `dhat-heap.json` on drop. [`alloc`] holds the returned guard
/// until after the last step, then drops it explicitly — `process::exit`
/// skips `Drop`.
fn profiler(dump: bool) -> dhat::Profiler {
    if dump {
        dhat::Profiler::new_heap()
    } else {
        dhat::Profiler::builder().testing().build()
    }
}

/// What a step is allowed to allocate.
#[derive(Clone, Copy, Debug)]
enum Limit {
    /// Not one block. palantir's own code, where the posture is an
    /// invariant rather than a budget.
    Zero,
    /// At most this many blocks per frame — a floor owned by someone
    /// else (the wgpu driver), so the gate catches drift, not presence.
    BlocksPerFrame(u64),
}

/// One step's measured window.
#[derive(Clone, Copy, Debug)]
struct Step {
    name: &'static str,
    blocks: u64,
    bytes: u64,
    limit: Limit,
}

impl Step {
    /// Warm up until retained scratch and caches stabilize, then count
    /// what `MEASURE_FRAMES` of steady state allocate.
    ///
    /// Too short a warmup is safe in the direction that matters: the
    /// leftovers land inside the measured window and trip the gate,
    /// rather than hiding under it. Measured, the fixture stabilizes by
    /// frame 4 — at 1 it still leaks ~10 blocks — so 16 is margin.
    fn measure(name: &'static str, limit: Limit, mut frame: impl FnMut()) -> Step {
        const WARMUP_FRAMES: usize = 16;

        for _ in 0..WARMUP_FRAMES {
            frame();
        }
        let before = dhat::HeapStats::get();
        for _ in 0..MEASURE_FRAMES {
            frame();
        }
        let after = dhat::HeapStats::get();
        Step {
            name,
            blocks: after.total_blocks - before.total_blocks,
            bytes: after.total_bytes - before.total_bytes,
            limit,
        }
    }

    fn blocks_per_frame(&self) -> f64 {
        self.blocks as f64 / MEASURE_FRAMES as f64
    }

    /// Blocks alone — `dhat` only ever adds to `total_bytes` alongside
    /// `total_blocks`, so a byte check could never fire on its own.
    fn over_limit(&self) -> bool {
        match self.limit {
            Limit::Zero => self.blocks != 0,
            Limit::BlocksPerFrame(max) => self.blocks > max * MEASURE_FRAMES as u64,
        }
    }

    fn report(&self) {
        let limit = match self.limit {
            Limit::Zero => "limit strict zero".to_owned(),
            Limit::BlocksPerFrame(max) => format!("limit <= {max}/frame"),
        };
        println!(
            "  {:<18} {:6} blocks  {:10} bytes  ({:6.2}/frame, {limit})",
            self.name,
            self.blocks,
            self.bytes,
            self.blocks_per_frame(),
        );
    }
}

/// Step 1 — palantir's record/measure/arrange/cascade/encode pipeline,
/// no GPU. Pins the `AGENTS.md` claim: "Per-frame allocation is a real
/// metric. Steady-state must be heap-alloc-free after warmup." Strict
/// zero, because everything in this path is ours.
///
/// Renders [`crate::ui::bench`]'s tree, at its surface and dpr, through
/// real cosmic shaping rather than the mono fallback — so what this gate
/// clears is the tree that bench times, not a smaller stand-in whose
/// quieter caches prove less.
///
/// Coverage stops where `Ui::frame` does, at damage: the encode and
/// compose passes need a `Frontend`, which `ui::bench` supplies through
/// its own `CpuHarness`. Step 2 runs them as part of a whole frame, and
/// `tests/alloc/fixtures/renderer.rs` audits them directly — replicating
/// `CpuHarness` here would buy a third copy of coverage that already
/// exists twice.
fn record_only() -> Step {
    let mut h = UiHarness::with_text(CACHED_SIZE).scale(SCALE);
    let mut state = FrameFixture::default();
    Step::measure("record-only", Limit::Zero, || {
        black_box(h.frame(|ui| state.render(BENCH_SCALE, ui)));
    })
}

// Driver floor on the current wgpu/cosmic-text pin. Bump if a driver
// upgrade or a deliberate palantir change moves the baseline; trip
// the gate otherwise. All current attribution is wgpu_core/wgpu_hal —
// no palantir-side per-frame allocs in this path.
const RENDER_BLOCKS_PER_FRAME_MAX: u64 = 35;

// The render step's own surface and tree, deliberately smaller than
// step 1's: what it pins is the driver's per-frame floor, which scales
// with submissions rather than with node count. A bigger tree would
// only make the same number slower to reach.
const RENDER_SURFACE: UVec2 = UVec2::new(1280, 800);
const RENDER_NODE_SCALE: usize = 6;

#[derive(Debug)]
struct FixtureApp<'a> {
    state: &'a mut FrameFixture,
}

impl App for FixtureApp<'_> {
    fn record(&mut self, _win: crate::window::WindowToken, ui: &mut Ui) {
        self.state.render(RENDER_NODE_SCALE, ui);
    }
}

/// Step 2 — a frame through `OffscreenHost::frame_offscreen` against an
/// offscreen target, with a poll between frames so submitted work drains
/// before the next.
///
/// **Not** strict zero, and cannot be: every wgpu submission allocates a
/// `CommandEncoder` Arc, a `CommandBuffer` Arc, the queue's in-flight
/// `Vec` push, and per-pass scratch from `wgpu_hal`. The floor on this
/// fixture measures exactly 30 blocks/frame on the current pin — the
/// same in dev and bench profiles, so a debug-profile check is
/// trustworthy — all of it attributed to wgpu_core/wgpu_hal beneath
/// `frame_offscreen` (verified in dh_view via `--dump`). So the gate
/// catches *drift* from that floor: a palantir regression, or a
/// wgpu/cosmic-text bump worth looking at.
fn record_and_render() -> Step {
    // `Bare`: the timestamp queries the instrumented device carries
    // allocate per frame, which is the very thing this step counts.
    let gpu = BenchGpu::shared(Timing::Bare);
    // The public offscreen path always copies from its backbuffer, so
    // the floor pinned here excludes the direct-present path.
    let mut host = gpu.offscreen_builder().build();
    let mut state = FrameFixture::default();
    host.ui().theme_mut().window_clear = Color::TRANSPARENT;

    let target = gpu.target(RENDER_SURFACE, "palantir.alloc.render.target");

    Step::measure(
        "record + render",
        Limit::BlocksPerFrame(RENDER_BLOCKS_PER_FRAME_MAX),
        || {
            black_box(host.frame_offscreen(&target, SCALE, &mut FixtureApp { state: &mut state }));
            gpu.wait();
        },
    )
}

/// The allocation bench: every step, one profiler, one verdict.
///
/// Steps run to completion even when an earlier one is over its limit —
/// two numbers localize a regression, one plus an early exit does not.
pub(crate) fn alloc(dump: bool) {
    let profiler = profiler(dump);

    println!("alloc: measure={MEASURE_FRAMES} frames/step");
    let steps = [record_only(), record_and_render()];
    for step in &steps {
        step.report();
    }

    // Before any exit: `process::exit` skips `Drop`, and dropping is
    // what writes `dhat-heap.json` under `--dump`.
    drop(profiler);

    let over: Vec<&Step> = steps.iter().filter(|s| s.over_limit()).collect();
    if over.is_empty() {
        println!();
        println!("PASS: every allocation gate held.");
        return;
    }

    eprintln!();
    for step in &over {
        match step.limit {
            Limit::Zero => eprintln!(
                "FAIL: {} must be strictly allocation-free; got {:.2} blocks/frame.",
                step.name,
                step.blocks_per_frame(),
            ),
            Limit::BlocksPerFrame(max) => eprintln!(
                "FAIL: {} exceeds the wgpu driver baseline ({:.2} > {max} blocks/frame).",
                step.name,
                step.blocks_per_frame(),
            ),
        }
    }
    eprintln!();
    eprintln!("Inspect call sites with:");
    eprintln!("  cargo bench --bench alloc --features bench -- --dump");
    eprintln!("  open dhat-heap.json at https://nnethercote.github.io/dh_view/");
    eprintln!("For per-frame attribution with backtraces, cargo test --test alloc.");
    if over
        .iter()
        .any(|s| matches!(s.limit, Limit::BlocksPerFrame(_)))
    {
        eprintln!();
        eprintln!("If the driver baseline legitimately moved (wgpu/cosmic-text upgrade,");
        eprintln!("intentional palantir change), bump RENDER_BLOCKS_PER_FRAME_MAX here.");
    }
    std::process::exit(1);
}
