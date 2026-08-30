//! Per-frame aggregate benchmark — two cleanly-separated benches in one
//! file, selected by the [`Arms`] the runner hands [`bench`](fn@bench)
//! (`cpu` / `gpu` / `both`):
//!
//! - **`bench_cpu`** (`frame/*_cpu`) — palantir's CPU pipeline in
//!   isolation, driven on a **bare `Ui` + standalone `Frontend` with no
//!   wgpu device at all** (same deviceless path as the alloc bench's
//!   `record-only` step). Each
//!   iter runs record → measure → arrange → cascade → damage → encode +
//!   compose and acks the present; nothing touches the GPU. This is the
//!   clean signal: no queue submit, no `device.poll` ioctl, no
//!   per-size framebuffer reconfiguration. Going through the offscreen renderer
//!   driver plus a poll charges every iter driver work that
//!   profiles as NVIDIA / kernel self-time — ~20% on
//!   `cached_cpu` and ~50% on `resizing_cpu` (multi-MB backbuffer
//!   reallocations per size) — swamping the palantir cost being measured.
//! - **`bench_gpu`** (`frame/*_gpu`) — the full public path:
//!   `OffscreenHost::frame_offscreen` against an offscreen `wgpu::Texture` +
//!   `PollType::Wait`. Wall time covers the whole CPU + GPU pipeline;
//!   dominated by GPU exec on large views. The per-frame `write_stats`
//!   dump (upload counts, GPU pass timings) lives here since it's
//!   inherently GPU.
//!
//! Running `--arms cpu` executes **zero** GPU code (no adapter/device
//! request, no `write_stats`), so a `perf` / `samply` capture of the CPU
//! bench is uncontaminated by driver activity.
//!
//! The three arms are shared in spirit across both benches:
//!
//! - **`frame/cached_*`** — fixed viewport, MeasureCache hits, damage
//!   resolves to `Skip` in steady state. The `_cpu` arm still runs a
//!   full-tree encode + compose (a synthesized `Full` plan) so it
//!   measures the same pipeline as the other arms rather than skipping
//!   paint; see `CpuHarness::frame`.
//! - **`frame/partial_*`** — fixed viewport, mutates a single fixture
//!   counter per iter so damage resolves to one small `Partial` rect
//!   over an otherwise-static tree. Models the steady-state of an
//!   interactive UI (animating counter / blinking caret / hover).
//! - **`frame/resizing_*`** — rotates a pool of differently-sized
//!   surfaces so `available_q` busts the measure cache each iter.
//! - **`frame/scrolling_*`** — fixed viewport, shifts a `Panel::transform`
//!   each iter so only the cascade walk sees change.
//!
//! After all selected arms run, each arm's criterion `time:` estimate
//! (the slope it reports to stdout) is prepended to
//! `benches/results/<machine>.txt` so per-machine history
//! is captured automatically. `--machine` overrides the filename derived
//! from `hostname -s`, and `--note` captions the row.
//!
//! `--size <w>x<h>` and `--scale <dpr>` override the surface every arm
//! renders into (the resize pool rescales with it), so the same fixture
//! can be measured at another display size without editing this file.
//! Culling is what makes the size matter: the fixture is taller than the
//! 1440p default view, so the CPU arms record, measure and arrange the
//! whole tree while paint and the GPU arms see only what is on screen. A
//! taller surface therefore costs more, not less.
//!
//! All four arrive in [`Run::fixture`] — this bench reads no environment
//! of its own.
//!
//! The shared workload lives in [`crate::frame_fixture`] and also drives
//! the allocation gates in `tests/alloc/gates.rs` and the showcase's
//! `frame bench` page — run `cargo run --bin showcase --features showcase`
//! to eyeball the tree these numbers come from.

use crate::app::internals::RecordApp;
use crate::bench::{Arms, Fixture, Run};
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};
use crate::host::bench_gpu::{BenchGpu, Timing};
use crate::host::offscreen::OffscreenHost;
use crate::primitives::color::Color;
use crate::renderer::backend::texture_region::counters::WriteStats;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::ui::harness::UiHarness;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};
use std::fs::OpenOptions;
use std::hint::black_box;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// Surface clear colour. Set on `theme.window_clear` in both harnesses
// and reused as the `clear` for the synthesized `Full` plan the CPU
// `cached` arm encodes against (see `CpuHarness::frame`).
const WINDOW_CLEAR: Color = Color::BLACK;
// Proportioned against `BENCH_SURFACE` — `Surface::new` rescales them by
// whatever ratio `--size` asks for, so what matters is the spread
// (-16%..+8% wide, -7%..+3% tall), not the absolute values. Multiples of
// 16 so a resized surface stays tile-aligned.
const RESIZE_POOL: &[glam::UVec2] = &[
    glam::UVec2::new(2144, 1344),
    glam::UVec2::new(2560, 1440),
    glam::UVec2::new(2352, 1392),
    glam::UVec2::new(2768, 1488),
];

/// [`Fixture`]'s options resolved against this bench's defaults — what
/// every arm actually renders into. Built once per run and threaded
/// down, so no arm re-derives it and the resize pool is scaled once
/// rather than at each of its four use sites.
#[derive(Clone, Debug)]
struct Surface {
    size: glam::UVec2,
    scale: f32,
    /// [`RESIZE_POOL`] scaled to keep its proportions against `size`.
    pool: Vec<glam::UVec2>,
}

impl Surface {
    fn new(fixture: Fixture<'_>) -> Self {
        let size = fixture.size.unwrap_or(BENCH_SURFACE);
        let ratio = size.as_vec2() / BENCH_SURFACE.as_vec2();
        Surface {
            size,
            scale: fixture.scale.unwrap_or(BENCH_DPR),
            pool: RESIZE_POOL
                .iter()
                .map(|s| (s.as_vec2() * ratio).round().as_uvec2())
                .collect(),
        }
    }
}

fn gpu() -> &'static BenchGpu {
    let gpu = BenchGpu::shared(Timing::Instrumented);
    static ANNOUNCED: OnceLock<()> = OnceLock::new();
    ANNOUNCED.get_or_init(|| {
        eprintln!("[frame_bench] timing features: {}", gpu.timing_summary());
    });
    gpu
}

fn bench_host(g: &BenchGpu) -> OffscreenHost {
    g.offscreen_builder().collect_gpu_stats(true).build()
}

fn frame_offscreen(
    host: &mut OffscreenHost,
    target: &wgpu::Texture,
    scale_factor: f32,
    record: impl FnMut(&mut Ui),
) {
    let mut app = RecordApp::new(record);
    host.frame_offscreen(target, scale_factor, &mut app);
}

/// Deviceless CPU-pipeline harness: a bare `Ui` (bundled-font shaper)
/// plus a standalone `Frontend` sharing the `Ui`'s record store. One
/// `frame` runs record → measure → arrange → cascade → damage and then,
/// when the frame produced a render plan, encode + compose — **stopping
/// before any GPU submit**. No `wgpu::Device` is ever created, so the
/// `frame/*_cpu` arms profile as pure palantir CPU work.
///
/// Time is advanced from a real `Instant` exactly like `WindowDriver::cpu_frame`
/// (`self.start.elapsed()`) so paint-anim / tooltip wakes fire on the
/// same cadence as production — otherwise a frozen clock could classify
/// frames as `PaintOnly` and skip the record closure the arms depend on.
#[derive(Debug)]
struct CpuHarness {
    harness: UiHarness,
    frontend: Frontend,
    start: std::time::Instant,
}

impl CpuHarness {
    fn new(surface: &Surface) -> Self {
        let harness = UiHarness::with_text(surface.size).scale(surface.scale);
        let frontend = Frontend::for_test();
        let mut h = Self {
            harness,
            frontend,
            start: Instant::now(),
        };
        h.harness.ui.theme_mut().window_clear = WINDOW_CLEAR;
        h
    }

    /// Drive one full CPU frame and ack the present so
    /// the next frame's `take_frame_plan` matches what the host would see
    /// after a real submit (lets `cached` settle into `Skip`).
    ///
    /// Encode + compose run on **every** frame so all CPU arms measure
    /// the same pipeline. A steady-state `cached` frame resolves damage
    /// to `Skip` and so produces no render plan — in production the host
    /// would present the prior backbuffer and skip the encoder. Here we
    /// substitute a `Full` plan instead, so `cached_cpu` measures the
    /// whole-tree encode + compose cost rather than strictly less work
    /// than the other arms. `partial` keeps its small `Partial` region
    /// (the partial-encode path is its real workload); the substitution
    /// only kicks in when there's nothing to paint at all.
    fn frame(&mut self, record: impl FnMut(&mut Ui)) {
        let report = self.harness.at(self.start.elapsed()).frame(record);
        let plan = report.plan.unwrap_or(RenderPlan {
            clear: WINDOW_CLEAR,
            damage: Damage::Full,
        });
        // The deviceless CPU harness's `Frontend` carries the baseline
        // texture-dim cap from `for_test*` (the GpuView size ladder needs it).
        self.frontend.build(self.harness.ui.frame_scene(), plan);
    }
}

/// Shared CPU-arm scaffolding: build a fresh deviceless harness, run 4
/// warmup frames to settle caches, then hand criterion the same closure.
fn run_cpu_arm<F>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    leaf: &str,
    surface: &Surface,
    mut iter: F,
) where
    F: FnMut(&mut CpuHarness, &mut FrameFixture),
{
    let mut h = CpuHarness::new(surface);
    let mut state = FrameFixture::default();
    for _ in 0..4 {
        iter(&mut h, &mut state);
    }
    group.bench_function(leaf, |b| {
        b.iter(|| iter(&mut h, &mut state));
    });
}

fn cpu_cached(group: &mut BenchmarkGroup<'_, WallTime>, surface: &Surface) {
    run_cpu_arm(group, "cached_cpu", surface, |h, state| {
        h.frame(|ui| state.render(BENCH_SCALE, ui));
    });
}

fn cpu_partial(group: &mut BenchmarkGroup<'_, WallTime>, surface: &Surface) {
    assert_partial_invariant(surface);
    run_cpu_arm(group, "partial_cpu", surface, |h, state| {
        // Mutate before recording — same cadence as the scrolling /
        // resizing arms — so every arm sets up this frame's input then
        // records it, rather than relying on the prior iter's leftover.
        state.tick = state.tick.wrapping_add(1);
        h.frame(|ui| state.render(BENCH_SCALE, ui));
    });
}

fn cpu_scrolling(group: &mut BenchmarkGroup<'_, WallTime>, surface: &Surface) {
    run_cpu_arm(group, "scrolling_cpu", surface, |h, state| {
        // Wraparound after a viewport's worth of pixels so the
        // transform stays in-bounds. `scroll_offset` is `glam::Vec2`.
        state.scroll_offset.x = (state.scroll_offset.x + 1.5) % 256.0;
        state.scroll_offset.y = (state.scroll_offset.y + 0.7) % 256.0;
        h.frame(|ui| state.render(BENCH_SCALE, ui));
    });
}

fn cpu_resizing(group: &mut BenchmarkGroup<'_, WallTime>, surface: &Surface) {
    let mut idx = 0usize;
    let pool = surface.pool.clone();
    run_cpu_arm(group, "resizing_cpu", surface, move |h, state| {
        let size = pool[idx % pool.len()];
        idx = idx.wrapping_add(1);
        h.harness.resize(size);
        h.frame(|ui| state.render(BENCH_SCALE, ui));
    });
}

/// Pin the Partial invariant before the timing loop: prime a deviceless
/// harness for a couple of frames, then inspect `report.plan`. If this
/// ever silently regresses to `Full` (e.g. someone widens the text box
/// and the digits drift the surrounding panel hash), the bench would
/// still produce a number but be measuring the wrong thing.
fn assert_partial_invariant(surface: &Surface) {
    let mut h = CpuHarness::new(surface);
    let mut state = FrameFixture::default();
    for _ in 0..2 {
        h.frame(|ui| state.render(BENCH_SCALE, ui));
        state.tick = state.tick.wrapping_add(1);
    }
    let report = h
        .harness
        .at(h.start.elapsed())
        .frame(|ui| state.render(BENCH_SCALE, ui));
    assert_eq!(
        report.paint(),
        FramePaint::Partial,
        "fixture's footer-status counter must produce a small damage rect",
    );
}

/// Shared GPU-arm scaffolding: build a fresh `OffscreenHost`, run 4
/// warmup frames with `PollType::Wait`, then hand criterion the same
/// closure. Each arm's `iter` closure owns target selection and per-iter
/// state mutation.
fn run_gpu_arm<F>(group: &mut BenchmarkGroup<'_, WallTime>, leaf: &str, mut iter: F)
where
    F: FnMut(&mut OffscreenHost, &mut FrameFixture),
{
    let g = gpu();
    let mut host = bench_host(g);
    host.ui().theme_mut().window_clear = WINDOW_CLEAR;
    let mut state = FrameFixture::default();
    for _ in 0..4 {
        iter(&mut host, &mut state);
    }
    group.bench_function(leaf, |b| {
        b.iter(|| iter(&mut host, &mut state));
    });
    // Drain pipelined GPU work before the next bench function reuses
    // the device.
    g.wait();
}

fn gpu_cached(group: &mut BenchmarkGroup<'_, WallTime>, surface: &Surface) {
    let target = gpu().target(surface.size, "palantir.frame_bench.cached");
    let scale = surface.scale;
    run_gpu_arm(group, "cached_gpu", |host, state| {
        frame_offscreen(host, &target, scale, |ui| state.render(BENCH_SCALE, ui));
        gpu().wait();
        black_box(&target);
    });
}

fn gpu_partial(group: &mut BenchmarkGroup<'_, WallTime>, surface: &Surface) {
    let target = gpu().target(surface.size, "palantir.frame_bench.partial");
    let scale = surface.scale;
    run_gpu_arm(group, "partial_gpu", |host, state| {
        state.tick = state.tick.wrapping_add(1);
        frame_offscreen(host, &target, scale, |ui| state.render(BENCH_SCALE, ui));
        gpu().wait();
        black_box(&target);
    });
}

fn gpu_scrolling(group: &mut BenchmarkGroup<'_, WallTime>, surface: &Surface) {
    let target = gpu().target(surface.size, "palantir.frame_bench.scrolling");
    let scale = surface.scale;
    run_gpu_arm(group, "scrolling_gpu", |host, state| {
        state.scroll_offset.x = (state.scroll_offset.x + 1.5) % 256.0;
        state.scroll_offset.y = (state.scroll_offset.y + 0.7) % 256.0;
        frame_offscreen(host, &target, scale, |ui| state.render(BENCH_SCALE, ui));
        gpu().wait();
        black_box(&target);
    });
}

fn gpu_resizing(group: &mut BenchmarkGroup<'_, WallTime>, surface: &Surface) {
    let targets: Vec<wgpu::Texture> = surface
        .pool
        .iter()
        .enumerate()
        .map(|(i, s)| gpu().target(*s, &format!("palantir.frame_bench.resize.{i}")))
        .collect();
    let mut idx = 0usize;
    let scale = surface.scale;
    run_gpu_arm(group, "resizing_gpu", move |host, state| {
        let t = &targets[idx % targets.len()];
        idx = idx.wrapping_add(1);
        frame_offscreen(host, t, scale, |ui| state.render(BENCH_SCALE, ui));
        gpu().wait();
        black_box(t);
    });
}

/// Per-frame `queue.write_*` counts + GPU main-pass time for each
/// arm, frames 0..=5, so the cold→warm transition is visible.
/// Upload columns come from the backend's counting queue instrumentation;
/// the GPU pass column comes from `wgpu` timestamp queries surfaced via
/// [`crate::GpuPassStats`].
/// The pass readout is one frame lagged (the `map_async` callback
/// fires after the next `device.poll`), so frame 0's column is
/// omitted.
fn report_write_stats(surface: &Surface) {
    fn run(
        label: &str,
        scale: f32,
        targets: &[wgpu::Texture],
        mut mutate: impl FnMut(&mut FrameFixture, usize),
    ) {
        let g = gpu();
        let mut host = bench_host(g);
        host.ui().theme_mut().window_clear = WINDOW_CLEAR;
        let mut state = FrameFixture::default();
        eprintln!("[write_stats] {label}:");
        for frame in 0..6 {
            mutate(&mut state, frame);
            let _ = WriteStats::take();
            let target = &targets[frame % targets.len()];
            frame_offscreen(&mut host, target, scale, |ui| state.render(BENCH_SCALE, ui));
            g.wait();
            let s = WriteStats::take();
            // The pass-time readout lags by one frame (the
            // `map_async` callback that publishes a value fires off
            // the *next* `device.poll`). One extra Poll here drains
            // the just-submitted frame's resolve so the column
            // matches the iteration we're printing rather than the
            // previous one.
            let _ = g.device.poll(wgpu::PollType::Poll);
            let stats = host.gpu_pass_stats();
            let gpu = stats
                .last_pass_ms()
                .map(|ms| format!("{ms:>5.2} ms"))
                .unwrap_or_else(|| "  n/a   ".into());
            eprintln!(
                "  frame {frame}  texture: {:>2} calls, {:>9} B   gpu: {gpu}",
                s.texture_calls, s.texture_bytes,
            );
            // Per-kind attribution (TIMESTAMP_QUERY_INSIDE_PASSES) and
            // pipeline stats (PIPELINE_STATISTICS_QUERY). Print only
            // when at least one value resolved, so adapters that lack
            // the feature stay quiet.
            use strum::IntoEnumIterator;
            let per_kind: Vec<String> = BatchKind::iter()
                .filter_map(|k| stats.last_kind_ms(k).map(|ms| (k, ms)))
                .map(|(k, ms)| format!("{}={ms:.2}", k.label()))
                .collect();
            if !per_kind.is_empty() {
                eprintln!("           kinds: {}", per_kind.join(" "));
            }
            if let Some(p) = stats.last_pipeline_stats() {
                eprintln!(
                    "           pipeline: vs={} clip_in={} clip_out={} fs={}",
                    p.vertex_shader_invocations,
                    p.clipper_invocations,
                    p.clipper_primitives_out,
                    p.fragment_shader_invocations,
                );
            }
        }
    }

    let g = gpu();
    let scale = surface.scale;
    let cached = [g.target(surface.size, "write_stats.cached")];
    run("cached", scale, &cached, |_, _| {});

    let partial = [g.target(surface.size, "write_stats.partial")];
    run("partial", scale, &partial, |state, _| {
        state.tick = state.tick.wrapping_add(1);
    });

    let pool: Vec<wgpu::Texture> = surface
        .pool
        .iter()
        .enumerate()
        .map(|(i, s)| g.target(*s, &format!("write_stats.resize.{i}")))
        .collect();
    run("resizing", scale, &pool, |_, _| {});

    let scrolling = [g.target(surface.size, "write_stats.scrolling")];
    run("scrolling", scale, &scrolling, |state, _| {
        state.scroll_offset.x = (state.scroll_offset.x + 1.5) % 256.0;
        state.scroll_offset.y = (state.scroll_offset.y + 0.7) % 256.0;
    });
}

/// The workloads both halves run, in the order the results row lists
/// them.
const CATEGORIES: [&str; 4] = ["cached", "partial", "resizing", "scrolling"];

/// Arm ids criterion runs for a given mode, interleaved cpu/gpu per
/// category. Used by the per-machine results writer to know which
/// criterion estimate files to read after all arms have finished.
///
/// Built from the same namespace `bench_cpu` / `bench_gpu` register
/// under, so a renamed group cannot leave the writer looking for
/// estimates that were never written.
fn arm_names(run: Run<'_>) -> Vec<String> {
    let group = run.group_name();
    let mut v = Vec::with_capacity(CATEGORIES.len() * 2);
    for category in CATEGORIES {
        if run.arms.includes_cpu() {
            v.push(format!("{group}/{category}_cpu"));
        }
        if run.arms.includes_gpu() {
            v.push(format!("{group}/{category}_gpu"));
        }
    }
    v
}

/// CPU bench: the deviceless `frame/*_cpu` arms. Skipped wholesale when
/// `--arms gpu` so a GPU-only run executes no CPU-arm code (and, more
/// importantly, an `--arms cpu` run reaches this without `bench_gpu` having
/// touched the GPU at all — pristine for profiling).
fn bench_cpu(c: &mut Criterion, run: Run<'_>, surface: &Surface) {
    if !run.arms.includes_cpu() {
        return;
    }
    let mut group = run.group(c);
    cpu_cached(&mut group, surface);
    cpu_partial(&mut group, surface);
    cpu_resizing(&mut group, surface);
    cpu_scrolling(&mut group, surface);
    group.finish();
}

/// GPU bench: the full-pipeline `frame/*_gpu` arms plus the per-frame
/// `write_stats` dump. Skipped wholesale when `--arms cpu`.
fn bench_gpu(c: &mut Criterion, run: Run<'_>, surface: &Surface) {
    if !run.arms.includes_gpu() {
        return;
    }
    report_write_stats(surface);
    let mut group = run.group(c);
    gpu_cached(&mut group, surface);
    gpu_partial(&mut group, surface);
    gpu_resizing(&mut group, surface);
    gpu_scrolling(&mut group, surface);
    group.finish();
}

/// Results finalizer — runs last in [`bench()`], and only when the run
/// records. Reads criterion's reported estimate out of
/// `target/criterion/<group>/<arm>/new/estimates.json` for every arm the two
/// benches just ran and prepends the `[lower point upper]` triple — the
/// same slope/mean criterion's stdout prints — to a per-machine `.txt`.
/// Newest run lives at the top of the file (`head` gives the latest).
/// Separated from the benches so it observes every arm regardless of
/// mode, and so neither bench has to know it's the last one.
fn prepend_machine_results(run: Run<'_>) {
    let machine = machine_label(run.fixture.machine);
    let mut block = String::new();
    let mode_tag = match run.arms {
        Arms::Cpu => "cpu",
        Arms::Gpu => "gpu",
        Arms::Both => "both",
    };
    block.push_str(&format!(
        "=== {} — [{}] {} ===\n",
        now_label(),
        mode_tag,
        bench_annotation(run.fixture.note)
    ));
    for name in arm_names(run) {
        let name = name.as_str();
        let row = match read_criterion_estimate(name) {
            Some(e) => format!("{name:<22} time: {}\n", fmt_estimate(e)),
            None => format!("{name:<22} time: (criterion estimates not found)\n"),
        };
        block.push_str(&row);
    }
    block.push('\n');

    prepend_block(Path::new("benches/results"), &machine, &block);
}

/// Put `block` at the top of `<dir>/<machine>.txt`, keeping whatever was
/// there below it. Best-effort: any I/O failure prints to stderr and
/// continues, because losing a history row must not fail a bench that
/// has already produced its numbers.
///
/// Split from [`prepend_machine_results`] so the file handling is
/// reachable from a test with a directory of its own — the caller's
/// `benches/results` is a fixed relative path a test cannot redirect.
fn prepend_block(dir: &Path, machine: &str, block: &str) {
    // The directory is gitignored, so a fresh checkout has none and the
    // tempfile open below would fail with ENOENT.
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("[machine-results] create {}: {e}", dir.display());
        return;
    }
    let path = dir.join(format!("{machine}.txt"));
    let prior = std::fs::read_to_string(&path).unwrap_or_default();
    // Atomic-enough rewrite: write to a sibling tempfile then rename
    // over the destination. Avoids leaving the file half-written if
    // the bench is interrupted mid-write.
    let tmp_path = path.with_extension("txt.tmp");
    let mut f = match OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[machine-results] open {}: {e}", tmp_path.display());
            return;
        }
    };
    if let Err(e) = f
        .write_all(block.as_bytes())
        .and_then(|_| f.write_all(prior.as_bytes()))
    {
        eprintln!("[machine-results] write {}: {e}", tmp_path.display());
        return;
    }
    drop(f);
    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        eprintln!(
            "[machine-results] rename {} -> {}: {e}",
            tmp_path.display(),
            path.display()
        );
        return;
    }
    eprintln!("[machine-results] prepended to {}", path.display());
}

#[derive(Debug, Clone, Copy)]
struct Estimate {
    lo_ns: f64,
    mid_ns: f64,
    hi_ns: f64,
}

/// Locate criterion's output root — the `criterion/` dir under the
/// `target/` cargo actually built into. The reliable signal is the bench
/// binary's own path: criterion writes under the same `target/` tree the
/// binary lives in (`<target>/<profile>/deps/<bin>`), and in this
/// workspace that's the shared `Scenarium/target`, NOT the submodule-local
/// `palantir/target`.
///
/// A CWD walk-up (the previous approach) is wrong: cargo runs the bench
/// with CWD at the submodule package dir, and a stale
/// `palantir/target/criterion` left by an earlier standalone build
/// shadows the real workspace dir — so the finalizer read months-old
/// estimates from it and every per-machine row was stale.
fn criterion_root() -> PathBuf {
    if let Ok(t) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(t).join("criterion");
    }
    // `current_exe()` = `<target>/<profile>/deps/<bin>`; the `target`
    // ancestor is the first one named "target" (robust to the profile
    // dir being release / debug / a custom name). `ancestors()` runs
    // deepest-first, so this lands on the real cargo target, never a
    // coincidental "target" higher in the path.
    if let Ok(exe) = std::env::current_exe()
        && let Some(target) = exe
            .ancestors()
            .find(|a| a.file_name() == Some("target".as_ref()))
    {
        return target.join("criterion");
    }
    // Last resort: CWD-relative, matching criterion's own fallback.
    PathBuf::from("target").join("criterion")
}

/// Extract the estimate criterion's stdout `time:` line reports, from its
/// `estimates.json`. Criterion prints the **slope** when it used
/// linear-regression sampling (the default — slope cancels per-iter
/// constant overhead and is the more accurate estimate for fast benches),
/// and falls back to the **mean** for flat sampling (`"slope":null`).
/// Mirror that order so the persisted row matches what criterion printed,
/// not a mean that reads ~1% high.
///
/// The file is a single-line JSON blob with a stable layout
/// (`"slope":{"confidence_interval":{...},"point_estimate":N,...}`): slice
/// into the named block and pick the three numbers in declaration order.
/// Avoids pulling serde_json just for this.
fn read_criterion_estimate(name: &str) -> Option<Estimate> {
    let s = std::fs::read_to_string(estimates_path(&criterion_root(), name)).ok()?;
    estimate_from_block(&s, "\"slope\":").or_else(|| estimate_from_block(&s, "\"mean\":"))
}

/// Where criterion filed `name`'s estimate: one directory per
/// `/`-separated component, which is how it lays a group out —
/// `criterion/<group>/<arm>/new/`. Flattening the separator into the
/// directory name instead names a path that never exists, and every row
/// files as "not found".
fn estimates_path(root: &Path, name: &str) -> PathBuf {
    name.split('/')
        .fold(root.to_path_buf(), |dir, part| dir.join(part))
        .join("new/estimates.json")
}

/// Read `{lower_bound, point_estimate, upper_bound}` out of the `key` block
/// (`"slope":` / `"mean":`). `None` for an absent block or `"slope":null`
/// (flat sampling) — without the null guard the number scan would walk
/// past it into the next block and report the wrong statistic.
fn estimate_from_block(s: &str, key: &str) -> Option<Estimate> {
    let after = &s[s.find(key)? + key.len()..];
    if after.trim_start().starts_with("null") {
        return None;
    }
    Some(Estimate {
        lo_ns: extract_json_number(after, "\"lower_bound\":")?,
        mid_ns: extract_json_number(after, "\"point_estimate\":")?,
        hi_ns: extract_json_number(after, "\"upper_bound\":")?,
    })
}

fn extract_json_number(s: &str, key: &str) -> Option<f64> {
    let i = s.find(key)? + key.len();
    let rest = &s[i..];
    let end = rest
        .find(|c: char| {
            !c.is_ascii_digit() && c != '.' && c != '-' && c != '+' && c != 'e' && c != 'E'
        })
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Render µs (sub-millisecond) or ms with two decimals, criterion
/// stdout-style. Auto-picks the unit per-value (a column may mix —
/// the median of `resizing_cpu` is ms while the CI radius is µs).
fn fmt_estimate(e: Estimate) -> String {
    fn one(ns: f64) -> String {
        let us = ns / 1_000.0;
        if us < 1000.0 {
            format!("{us:7.2} µs")
        } else {
            format!("{:7.3} ms", us / 1000.0)
        }
    }
    format!("[{} {} {}]", one(e.lo_ns), one(e.mid_ns), one(e.hi_ns))
}

/// `--machine` overrides the default hostname-derived label. Sanitized
/// to lowercase alnum + `-_` (first dotted component only, so FQDNs
/// collapse to their short form) so it's safe as a filename. Falls back
/// to `gethostname`; empty result → `unknown`.
fn machine_label(machine: Option<&str>) -> String {
    fn sanitize(raw: &str) -> String {
        raw.trim()
            .split('.')
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_lowercase()
    }
    if let Some(given) = machine {
        let n = sanitize(given);
        if !n.is_empty() {
            return n;
        }
    }
    let raw = gethostname::gethostname();
    let n = sanitize(&raw.to_string_lossy());
    if n.is_empty() { "unknown".into() } else { n }
}

/// Required context tag for the results row, from `--note`. The bench
/// refuses to run without one so every appended row has a
/// why-was-this-measured caption.
fn bench_annotation(note: Option<&str>) -> &str {
    match note.map(str::trim) {
        Some(s) if !s.is_empty() => s,
        _ => panic!(
            "frame bench requires a note; e.g. cargo bench --bench criterion \
             -- -d frame --note 'after staging-belt rework'",
        ),
    }
}

fn now_label() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%d %H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown-time".into())
}

// Longer per-arm measurement window than criterion's 5 s default —
// the GPU arms (`*_gpu`) bounce ±15-25% across back-to-back runs because
// thermals + scheduler noise share budget with everything else on the
// machine. Doubling the window roughly halves the run-to-run spread;
// total wall time goes from ~50 s to ~90 s, which is fine for an
// on-demand bench. `--arms cpu` skips `bench_gpu` outright, so a CPU run
// (and a profile of one) executes no GPU code at all; the results row is
// prepended last.
pub(crate) fn config() -> Criterion {
    Criterion::default()
        .measurement_time(Duration::from_secs(12))
        .warm_up_time(Duration::from_secs(3))
}

/// `arms` decides which half runs — the runner resolved it from the
/// driver's declared [`Arms::Both`] against what the invocation asked
/// for, so this no longer reads the environment or decides whether it
/// was wanted at all. Being called *is* being wanted; the registry's
/// `opt_in` keeps it out of the default set.
pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    // Test and profile modes write no estimate, so the results row —
    // and the note it demands — would be meaningless.
    //
    // Fail fast before any arm runs so a long bench doesn't finish and
    // then realise the results row has no context.
    if run.recording {
        let _ = bench_annotation(run.fixture.note);
    }
    let surface = Surface::new(run.fixture);
    bench_cpu(c, run, &surface);
    bench_gpu(c, run, &surface);
    if run.recording {
        prepend_machine_results(run);
    }
}

#[cfg(test)]
mod tests {
    use crate::bench::Fixture;
    use crate::frame_fixture::{BENCH_DPR, BENCH_SURFACE};
    use crate::ui::bench::{RESIZE_POOL, Surface, estimates_path, prepend_block};

    /// The results directory is gitignored, so the common case on a
    /// fresh checkout is that it does not exist — a writer that only
    /// opens the tempfile drops the row every run on such a machine.
    /// Also pins newest-on-top, which is what makes `head` the latest
    /// run.
    #[test]
    fn a_row_creates_the_missing_results_dir_and_lands_on_top() {
        let root =
            std::env::temp_dir().join(format!("palantir-bench-results-{}", std::process::id()));
        let dir = root.join("results");
        let _ = std::fs::remove_dir_all(&root);
        assert!(!dir.exists(), "the case under test is an absent dir");

        prepend_block(&dir, "rig", "older\n");
        prepend_block(&dir, "rig", "newer\n");

        let path = dir.join("rig.txt");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer\nolder\n");
        assert!(
            !dir.join("rig.txt.tmp").exists(),
            "the rename must leave no tempfile behind",
        );
        // A second machine writes beside the first, not over it.
        prepend_block(&dir, "other", "elsewhere\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer\nolder\n");

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// `--size` has to reach the pool as well as the cached arm, or the
    /// resize arm keeps rendering at the default while every other arm
    /// moves — the two would no longer be measuring the same fixture.
    #[test]
    fn a_given_size_scales_the_resize_pool_by_the_same_ratio() {
        let d = Surface::new(Fixture::default());
        assert_eq!(d.size, BENCH_SURFACE);
        assert_eq!(d.scale, BENCH_DPR);
        assert_eq!(d.pool, RESIZE_POOL, "unset size leaves the pool alone");

        // Half the default in both axes: 2560x1440 -> 1280x720, ratio
        // 0.5, so 2144x1344 -> 1072x672 and 2768x1488 -> 1384x744.
        let half = Surface::new(Fixture {
            size: Some(glam::UVec2::new(1280, 720)),
            scale: Some(1.0),
            ..Fixture::default()
        });
        assert_eq!(half.size, glam::UVec2::new(1280, 720));
        assert_eq!(half.scale, 1.0);
        assert_eq!(
            half.pool,
            [
                glam::UVec2::new(1072, 672),
                glam::UVec2::new(1280, 720),
                glam::UVec2::new(1176, 696),
                glam::UVec2::new(1384, 744),
            ],
        );

        // A non-integral ratio rounds rather than truncating: width
        // 2560 -> 2100 is 0.8203125, and 2144 * that = 1758.75 -> 1759.
        let odd = Surface::new(Fixture {
            size: Some(glam::UVec2::new(2100, 1440)),
            ..Fixture::default()
        });
        assert_eq!(odd.pool[0].x, 1759);
        assert_eq!(odd.scale, BENCH_DPR, "unset scale keeps the default");
    }

    /// Criterion nests a group: `criterion/frame/cached_gpu/new/`. The
    /// arm id carries the `/`, so the separator has to become a
    /// directory boundary and not part of one name.
    #[test]
    fn estimates_path_nests_the_group_and_arm() {
        let root = std::path::Path::new("/t/criterion");
        assert_eq!(
            estimates_path(root, "frame/cached_gpu"),
            root.join("frame")
                .join("cached_gpu")
                .join("new/estimates.json"),
        );
        assert_eq!(
            estimates_path(root, "solo"),
            root.join("solo").join("new/estimates.json"),
            "an id with no group is one directory deep",
        );
    }
}
