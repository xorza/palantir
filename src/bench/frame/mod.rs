//! Per-frame aggregate benchmark — two cleanly-separated benches in one
//! file, selected by `APERTURE_BENCH_MODE` (`cpu` / `gpu` / `both`):
//!
//! - **`bench_cpu`** (`frame/*_cpu`) — aperture's CPU pipeline in
//!   isolation, driven on a **bare `Ui` + standalone `Frontend` with no
//!   wgpu device at all** (same deviceless path as `alloc_free`). Each
//!   iter runs record → measure → arrange → cascade → damage → encode +
//!   compose and acks the present; nothing touches the GPU. This is the
//!   clean signal: no queue submit, no `device.poll` ioctl, no
//!   per-size framebuffer reconfiguration. Going through the offscreen renderer
//!   driver + a poll (the old shape) charged every iter driver work that
//!   profiled as NVIDIA / kernel self-time — ~20% on
//!   `cached_cpu` and ~50% on `resizing_cpu` (multi-MB backbuffer
//!   reallocations per size) — swamping the aperture cost being measured.
//! - **`bench_gpu`** (`frame/*_gpu`) — the full public path:
//!   `OffscreenHost::frame_offscreen` against an offscreen `wgpu::Texture` +
//!   `PollType::Wait`. Wall time covers the whole CPU + GPU pipeline;
//!   dominated by GPU exec on large views. The per-frame `write_stats`
//!   dump (upload counts, GPU pass timings) lives here since it's
//!   inherently GPU.
//!
//! Running `MODE=cpu` executes **zero** GPU code (no adapter/device
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
//! is captured automatically. `APERTURE_BENCH_MACHINE` overrides the
//! filename derived from `hostname -s`.
//!
//! The shared workload lives in [`fixture`] and also drives the allocation
//! benches and `examples/frame_visual.rs`.

pub(crate) mod fixture;

use crate::app::test_support::RecordApp;
use crate::bench::frame::fixture::{BENCH_SCALE, FrameFixture, build_ui};
use crate::diagnostics::gpu_stats::BatchKind;
use crate::display::Display;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::color::Color;
use crate::renderer::backend::write_stats;
use crate::renderer::frontend::Frontend;
use crate::renderer::plan::{RenderKind, RenderPlan};
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::window::WindowToken;
use criterion::Criterion;
use pollster::FutureExt;
use std::fs::OpenOptions;
use std::hint::black_box;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SCALE: f32 = 2.0;
// Surface clear colour. Set on `theme.window_clear` in both harnesses
// and reused as the `clear` for the synthesized `Full` plan the CPU
// `cached` arm encodes against (see `CpuHarness::frame`).
const WINDOW_CLEAR: Color = Color::BLACK;
// View sized so `BENCH_SCALE = 32` content (36-row prop grid, 96-button
// wrap, shape gallery, 96-dot canvas, chat scroll, notes) fits without
// overflowing the main column.
const CACHED_SIZE: glam::UVec2 = glam::UVec2::new(3840, 4800); // 1920x2400 @ 2x
const RESIZE_POOL: &[glam::UVec2] = &[
    glam::UVec2::new(3200, 4400),
    glam::UVec2::new(3840, 4800),
    glam::UVec2::new(3520, 4600),
    glam::UVec2::new(4160, 5000),
];

/// Block until the GPU has drained all submitted work. The `_gpu` arms
/// call this between iters so wall time covers the full CPU + GPU frame.
fn gpu_wait(device: &wgpu::Device) {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("device poll");
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu() -> &'static Gpu {
    static G: OnceLock<Gpu> = OnceLock::new();
    G.get_or_init(|| {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .block_on()
            .expect("request adapter (headless)");
        // Request the full instrumentation feature set so the
        // backend's `GpuTimings` can publish whole-pass + per-batch
        // durations + pipeline statistics. The intersection with
        // `adapter.features()` drops bits the adapter doesn't
        // advertise; missing features degrade individually. The
        // frame bench is `--features internals` only — the right
        // place to keep instrumentation on by default.
        let timing_features = adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
                | wgpu::Features::PIPELINE_STATISTICS_QUERY);
        eprintln!(
            "[frame_bench] timing features: TIMESTAMP_QUERY={} INSIDE_PASSES={} PIPELINE_STATS={}",
            timing_features.contains(wgpu::Features::TIMESTAMP_QUERY),
            timing_features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
            timing_features.contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
        );
        // Match the production host: text Params is carried via
        // immediates (push constants), so the feature + 16-byte
        // immediate budget are required.
        let mut limits = wgpu::Limits::default();
        limits.max_immediate_size = limits.max_immediate_size.max(16);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("aperture.frame_bench.device"),
                required_features: timing_features | wgpu::Features::IMMEDIATES,
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .expect("request device");
        Gpu { device, queue }
    })
}

/// Build an `OffscreenHost` (one shared renderer + one window) from
/// the shared bench device with GPU instrumentation on. Every bench arm
/// wants the same shape — bundled fonts and a timestamp-enabled device.
fn bench_host(g: &Gpu) -> OffscreenHost {
    OffscreenHost::builder(
        WindowToken(0),
        g.device.clone(),
        g.queue.clone(),
        TextShaper::with_bundled_fonts(),
    )
    .collect_gpu_stats(true)
    .build()
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

fn make_target(device: &wgpu::Device, size: glam::UVec2, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Deviceless CPU-pipeline harness: a bare `Ui` (bundled-font shaper)
/// plus a standalone `Frontend` sharing the `Ui`'s record store. One
/// `frame` runs record → measure → arrange → cascade → damage and then,
/// when the frame produced a render plan, encode + compose — **stopping
/// before any GPU submit**. No `wgpu::Device` is ever created, so the
/// `frame/*_cpu` arms profile as pure aperture CPU work.
///
/// Time is advanced from a real `Instant` exactly like `WindowDriver::cpu_frame`
/// (`self.start.elapsed()`) so paint-anim / tooltip wakes fire on the
/// same cadence as production — otherwise a frozen clock could classify
/// frames as `PaintOnly` and skip the record closure the arms depend on.
struct CpuHarness {
    ui: Ui,
    frontend: Frontend,
    start: std::time::Instant,
}

impl CpuHarness {
    fn new() -> Self {
        let ui = Ui::for_test_text();
        let frontend = Frontend::for_test();
        let mut h = Self {
            ui,
            frontend,
            start: Instant::now(),
        };
        h.ui.theme.window_clear = WINDOW_CLEAR;
        h
    }

    /// Drive one full CPU frame against `display` and ack the present so
    /// the next frame's `classify_frame` matches what the host would see
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
    fn frame(&mut self, display: Display, record: impl FnMut(&mut Ui)) {
        let report = self
            .ui
            .record_test_frame(display, self.start.elapsed(), record);
        let plan = report.plan.unwrap_or(RenderPlan {
            clear: WINDOW_CLEAR,
            kind: RenderKind::Full,
        });
        // The deviceless CPU harness's `Frontend` carries the baseline
        // texture-dim cap from `for_test*` (the GpuView size ladder needs it).
        self.frontend.build(self.ui.frame_scene(), plan);
    }
}

/// Shared CPU-arm scaffolding: build a fresh deviceless harness, run 4
/// warmup frames to settle caches, then hand criterion the same closure.
fn run_cpu_arm<F>(c: &mut Criterion, name: &str, mut iter: F)
where
    F: FnMut(&mut CpuHarness, &mut FrameFixture),
{
    let mut h = CpuHarness::new();
    let mut state = FrameFixture::default();
    for _ in 0..4 {
        iter(&mut h, &mut state);
    }
    c.bench_function(name, |b| {
        b.iter(|| iter(&mut h, &mut state));
    });
}

fn cpu_cached(c: &mut Criterion) {
    run_cpu_arm(c, "frame/cached_cpu", |h, state| {
        h.frame(Display::from_physical(CACHED_SIZE, SCALE), |ui| {
            build_ui(state, BENCH_SCALE, ui)
        });
    });
}

fn cpu_partial(c: &mut Criterion) {
    assert_partial_invariant();
    run_cpu_arm(c, "frame/partial_cpu", |h, state| {
        // Mutate before recording — same cadence as the scrolling /
        // resizing arms — so every arm sets up this frame's input then
        // records it, rather than relying on the prior iter's leftover.
        state.tick = state.tick.wrapping_add(1);
        h.frame(Display::from_physical(CACHED_SIZE, SCALE), |ui| {
            build_ui(state, BENCH_SCALE, ui)
        });
    });
}

fn cpu_scrolling(c: &mut Criterion) {
    run_cpu_arm(c, "frame/scrolling_cpu", |h, state| {
        // Wraparound after a viewport's worth of pixels so the
        // transform stays in-bounds. `scroll_offset` is `glam::Vec2`.
        state.scroll_offset.x = (state.scroll_offset.x + 1.5) % 256.0;
        state.scroll_offset.y = (state.scroll_offset.y + 0.7) % 256.0;
        h.frame(Display::from_physical(CACHED_SIZE, SCALE), |ui| {
            build_ui(state, BENCH_SCALE, ui)
        });
    });
}

fn cpu_resizing(c: &mut Criterion) {
    let mut idx = 0usize;
    run_cpu_arm(c, "frame/resizing_cpu", move |h, state| {
        let size = RESIZE_POOL[idx % RESIZE_POOL.len()];
        idx = idx.wrapping_add(1);
        h.frame(Display::from_physical(size, SCALE), |ui| {
            build_ui(state, BENCH_SCALE, ui)
        });
    });
}

/// Pin the Partial invariant before the timing loop: prime a deviceless
/// harness for a couple of frames, then inspect `report.plan`. If this
/// ever silently regresses to `Full` (e.g. someone widens the text box
/// and the digits drift the surrounding panel hash), the bench would
/// still produce a number but be measuring the wrong thing.
fn assert_partial_invariant() {
    let mut h = CpuHarness::new();
    let mut state = FrameFixture::default();
    let display = Display::from_physical(CACHED_SIZE, SCALE);
    for _ in 0..2 {
        h.frame(display, |ui| build_ui(&mut state, BENCH_SCALE, ui));
        state.tick = state.tick.wrapping_add(1);
    }
    let report =
        h.ui.record_test_frame(display, h.start.elapsed(), |ui| {
            build_ui(&mut state, BENCH_SCALE, ui)
        });
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
fn run_gpu_arm<F>(c: &mut Criterion, name: &str, mut iter: F)
where
    F: FnMut(&mut OffscreenHost, &mut FrameFixture, &wgpu::Device),
{
    let g = gpu();
    let mut host = bench_host(g);
    host.ui().theme.window_clear = WINDOW_CLEAR;
    let mut state = FrameFixture::default();
    for _ in 0..4 {
        iter(&mut host, &mut state, &g.device);
    }
    c.bench_function(name, |b| {
        b.iter(|| iter(&mut host, &mut state, &g.device));
    });
    // Drain pipelined GPU work before the next bench function reuses
    // the device.
    gpu_wait(&g.device);
}

fn gpu_cached(c: &mut Criterion) {
    let target = make_target(&gpu().device, CACHED_SIZE, "aperture.frame_bench.cached");
    run_gpu_arm(c, "frame/cached_gpu", |host, state, device| {
        frame_offscreen(host, &target, SCALE, |ui| build_ui(state, BENCH_SCALE, ui));
        gpu_wait(device);
        black_box(&target);
    });
}

fn gpu_partial(c: &mut Criterion) {
    let target = make_target(&gpu().device, CACHED_SIZE, "aperture.frame_bench.partial");
    run_gpu_arm(c, "frame/partial_gpu", |host, state, device| {
        state.tick = state.tick.wrapping_add(1);
        frame_offscreen(host, &target, SCALE, |ui| build_ui(state, BENCH_SCALE, ui));
        gpu_wait(device);
        black_box(&target);
    });
}

fn gpu_scrolling(c: &mut Criterion) {
    let target = make_target(&gpu().device, CACHED_SIZE, "aperture.frame_bench.scrolling");
    run_gpu_arm(c, "frame/scrolling_gpu", |host, state, device| {
        state.scroll_offset.x = (state.scroll_offset.x + 1.5) % 256.0;
        state.scroll_offset.y = (state.scroll_offset.y + 0.7) % 256.0;
        frame_offscreen(host, &target, SCALE, |ui| build_ui(state, BENCH_SCALE, ui));
        gpu_wait(device);
        black_box(&target);
    });
}

fn gpu_resizing(c: &mut Criterion) {
    let targets: Vec<wgpu::Texture> = RESIZE_POOL
        .iter()
        .enumerate()
        .map(|(i, s)| {
            make_target(
                &gpu().device,
                *s,
                &format!("aperture.frame_bench.resize.{i}"),
            )
        })
        .collect();
    let mut idx = 0usize;
    run_gpu_arm(c, "frame/resizing_gpu", move |host, state, device| {
        let t = &targets[idx % targets.len()];
        idx = idx.wrapping_add(1);
        frame_offscreen(host, t, SCALE, |ui| build_ui(state, BENCH_SCALE, ui));
        gpu_wait(device);
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
fn report_write_stats() {
    fn run(
        label: &str,
        targets: &[wgpu::Texture],
        mut mutate: impl FnMut(&mut FrameFixture, usize),
    ) {
        let g = gpu();
        let mut host = bench_host(g);
        host.ui().theme.window_clear = WINDOW_CLEAR;
        let mut state = FrameFixture::default();
        eprintln!("[write_stats] {label}:");
        for frame in 0..6 {
            mutate(&mut state, frame);
            let _ = write_stats::take();
            let target = &targets[frame % targets.len()];
            frame_offscreen(&mut host, target, SCALE, |ui| {
                build_ui(&mut state, BENCH_SCALE, ui)
            });
            gpu_wait(&g.device);
            let s = write_stats::take();
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
    let cached = [make_target(&g.device, CACHED_SIZE, "write_stats.cached")];
    run("cached", &cached, |_, _| {});

    let partial = [make_target(&g.device, CACHED_SIZE, "write_stats.partial")];
    run("partial", &partial, |state, _| {
        state.tick = state.tick.wrapping_add(1);
    });

    let pool: Vec<wgpu::Texture> = RESIZE_POOL
        .iter()
        .enumerate()
        .map(|(i, s)| make_target(&g.device, *s, &format!("write_stats.resize.{i}")))
        .collect();
    run("resizing", &pool, |_, _| {});

    let scrolling = [make_target(&g.device, CACHED_SIZE, "write_stats.scrolling")];
    run("scrolling", &scrolling, |state, _| {
        state.scroll_offset.x = (state.scroll_offset.x + 1.5) % 256.0;
        state.scroll_offset.y = (state.scroll_offset.y + 0.7) % 256.0;
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BenchMode {
    Cpu,
    Gpu,
    Both,
}

impl BenchMode {
    fn includes_cpu(self) -> bool {
        matches!(self, BenchMode::Cpu | BenchMode::Both)
    }
    fn includes_gpu(self) -> bool {
        matches!(self, BenchMode::Gpu | BenchMode::Both)
    }
}

/// Required mode selector for the frame bench. Read from
/// `APERTURE_BENCH_MODE`; accepts `cpu`, `gpu`, or `both`. The bench
/// refuses to run without one so every invocation is an explicit
/// decision about which arms to pay for (the full `both` matrix is
/// ~90 s; `cpu` or `gpu` alone is ~45 s).
fn bench_mode() -> BenchMode {
    match std::env::var("APERTURE_BENCH_MODE")
        .ok()
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("cpu") => BenchMode::Cpu,
        Some("gpu") => BenchMode::Gpu,
        Some("both") => BenchMode::Both,
        _ => panic!(
            "frame bench requires APERTURE_BENCH_MODE=cpu|gpu|both; \
             e.g. APERTURE_BENCH_MODE=cpu APERTURE_BENCH_NOTE='...' cargo bench --bench frame",
        ),
    }
}

/// Arm names criterion runs for a given mode, interleaved cpu/gpu per
/// category. Used by the per-machine results writer to know which
/// criterion estimate files to read after all arms have finished.
fn arm_names(mode: BenchMode) -> Vec<&'static str> {
    let mut v = Vec::with_capacity(6);
    if mode.includes_cpu() {
        v.push("frame/cached_cpu");
    }
    if mode.includes_gpu() {
        v.push("frame/cached_gpu");
    }
    if mode.includes_cpu() {
        v.push("frame/partial_cpu");
    }
    if mode.includes_gpu() {
        v.push("frame/partial_gpu");
    }
    if mode.includes_cpu() {
        v.push("frame/resizing_cpu");
    }
    if mode.includes_gpu() {
        v.push("frame/resizing_gpu");
    }
    if mode.includes_cpu() {
        v.push("frame/scrolling_cpu");
    }
    if mode.includes_gpu() {
        v.push("frame/scrolling_gpu");
    }
    v
}

/// CPU bench: the deviceless `frame/*_cpu` arms. Skipped wholesale when
/// `MODE=gpu` so a GPU-only run executes no CPU-arm code (and, more
/// importantly, a `MODE=cpu` run reaches this without `bench_gpu` having
/// touched the GPU at all — pristine for profiling).
fn bench_cpu(c: &mut Criterion) {
    // Fail fast before any work runs so a long bench doesn't finish and
    // then realise the results row has no context.
    let _ = bench_annotation();
    if !bench_mode().includes_cpu() {
        return;
    }
    cpu_cached(c);
    cpu_partial(c);
    cpu_resizing(c);
    cpu_scrolling(c);
}

/// GPU bench: the full-pipeline `frame/*_gpu` arms plus the per-frame
/// `write_stats` dump. Skipped wholesale when `MODE=cpu`.
fn bench_gpu(c: &mut Criterion) {
    let _ = bench_annotation();
    if !bench_mode().includes_gpu() {
        return;
    }
    report_write_stats();
    gpu_cached(c);
    gpu_partial(c);
    gpu_resizing(c);
    gpu_scrolling(c);
}

/// Results finalizer — runs last in [`bench`]. Reads the
/// criterion `time:` estimates the two benches just wrote and prepends a
/// per-machine results row. Separated from the benches so it observes
/// every arm regardless of mode, and so neither bench has to know it's
/// the last one.
fn write_results(_c: &mut Criterion) {
    prepend_machine_results(bench_mode());
}

/// Read criterion's reported estimate out of `target/criterion/<slug>/new/estimates.json`
/// and write the `[lower point upper]` triple — the same slope/mean
/// criterion's stdout prints — to a per-machine `.txt`. Newest run lives
/// at the top of the file (`head` gives the latest). Best-effort: any I/O
/// failure prints to stderr and continues.
fn prepend_machine_results(mode: BenchMode) {
    let machine = machine_label();
    let path = PathBuf::from("benches/results").join(format!("{machine}.txt"));
    let mut block = String::new();
    let mode_tag = match mode {
        BenchMode::Cpu => "cpu",
        BenchMode::Gpu => "gpu",
        BenchMode::Both => "both",
    };
    block.push_str(&format!(
        "=== {} — [{}] {} ===\n",
        now_label(),
        mode_tag,
        bench_annotation()
    ));
    for &name in arm_names(mode).iter() {
        let row = match read_criterion_estimate(name) {
            Some(e) => format!("{name:<22} time: {}\n", fmt_estimate(e)),
            None => format!("{name:<22} time: (criterion estimates not found)\n"),
        };
        block.push_str(&row);
    }
    block.push('\n');

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

#[derive(Clone, Copy)]
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
/// `aperture/target`.
///
/// A CWD walk-up (the previous approach) is wrong: cargo runs the bench
/// with CWD at the submodule package dir, and a stale
/// `aperture/target/criterion` left by an earlier standalone build
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
    let slug = name.replace('/', "_");
    let path = criterion_root().join(&slug).join("new/estimates.json");
    let s = std::fs::read_to_string(&path).ok()?;
    estimate_from_block(&s, "\"slope\":").or_else(|| estimate_from_block(&s, "\"mean\":"))
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

/// `APERTURE_BENCH_MACHINE` overrides the default hostname-derived
/// label. Sanitized to lowercase alnum + `-_` (first dotted component
/// only, so FQDNs collapse to their short form) so it's safe as a
/// filename. Falls back to `gethostname`; empty result → `unknown`.
fn machine_label() -> String {
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
    if let Ok(env) = std::env::var("APERTURE_BENCH_MACHINE") {
        let n = sanitize(&env);
        if !n.is_empty() {
            return n;
        }
    }
    let raw = gethostname::gethostname();
    let n = sanitize(&raw.to_string_lossy());
    if n.is_empty() { "unknown".into() } else { n }
}

/// Required context tag for the results row. Read from
/// `APERTURE_BENCH_NOTE`; the bench refuses to run without one so
/// every appended row has a why-was-this-measured caption.
fn bench_annotation() -> String {
    match std::env::var("APERTURE_BENCH_NOTE") {
        Ok(s) if !s.trim().is_empty() => s.trim().to_owned(),
        _ => panic!(
            "frame bench requires APERTURE_BENCH_NOTE=<short context>; \
             e.g. APERTURE_BENCH_NOTE='after staging-belt rework' cargo bench --bench frame",
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
// on-demand bench. `cpu` and `gpu` are separate criterion groups so
// `MODE=cpu` can run (and be profiled) without any GPU code executing;
// `results` runs last to prepend the per-machine row.
pub fn config() -> Criterion {
    Criterion::default()
        .measurement_time(Duration::from_secs(12))
        .warm_up_time(Duration::from_secs(3))
}

pub fn text_ui() -> Ui {
    Ui::for_test_text()
}

pub fn bench(c: &mut Criterion) {
    bench_cpu(c);
    bench_gpu(c);
    write_results(c);
}
