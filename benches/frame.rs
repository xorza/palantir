//! Per-frame aggregate benchmark — CPU + GPU.
//!
//! Drives the canonical public API: `Host::frame_offscreen` against an
//! offscreen `wgpu::Texture`. Three arms × two sync modes:
//!
//! - **`frame/cached_*`** — fixed viewport, MeasureCache hits.
//! - **`frame/partial_*`** — fixed viewport, mutates a single fixture
//!   counter per iter so damage resolves to one small `Partial` rect
//!   over an otherwise-static tree. Models the steady-state of an
//!   interactive UI (animating counter / blinking caret / hover).
//! - **`frame/resizing_*`** — rotates a pool of differently-sized
//!   targets so `available_q` busts each iter.
//!
//! Each arm runs in both sync modes:
//!
//! - **`*_gpu`** — `PollType::Wait` between iters. Wall time covers
//!   the full CPU + GPU pipeline. Useful as the "what does a frame
//!   actually cost end-to-end" number; dominated by GPU exec on
//!   large views.
//! - **`*_cpu`** — `PollType::Poll` (non-blocking). Wall time covers
//!   record + measure + arrange + cascade + encode + compose + the
//!   CPU side of submit, but not GPU exec. Useful for measuring
//!   palantir's CPU pipeline without GPU variance dominating.
//!
//! After all arms run, each arm's criterion `mean` estimate is
//! prepended to `benches/results/<machine>.txt` so per-machine history
//! is captured automatically. `PALANTIR_BENCH_MACHINE` overrides the
//! filename derived from `hostname -s`.
//!
//! The `build_ui` workload lives in `benches/support/frame_fixture.rs`
//! and is shared with `examples/frame_visual.rs`.

#[path = "support/frame_fixture.rs"]
mod fixture;

use criterion::{Criterion, criterion_group, criterion_main};
use fixture::{BENCH_SCALE, FormState, build_ui};
use palantir::ui::frame_report::RenderPlan;
use palantir::{Color, Display, Host};
use pollster::FutureExt;
use std::fs::OpenOptions;
use std::hint::black_box;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SCALE: f32 = 2.0;
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

#[derive(Clone, Copy)]
enum SyncMode {
    /// Block on GPU completion between iters. Wall time = full
    /// CPU + GPU frame.
    Gpu,
    /// Non-blocking poll between iters. Wall time = CPU pipeline only;
    /// GPU work runs async and isn't counted.
    Cpu,
}

impl SyncMode {
    fn poll(self, device: &wgpu::Device) {
        let pt = match self {
            SyncMode::Gpu => wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            },
            SyncMode::Cpu => wgpu::PollType::Poll,
        };
        device.poll(pt).expect("device poll");
    }
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
            })
            .block_on()
            .expect("request adapter (headless)");
        // Mirror the showcase host: request `TIMESTAMP_QUERY` when
        // the adapter advertises it so the backend's
        // `GpuTimings` runs and `gpu_pass_stats::last_pass_ms()`
        // returns real values. The frame bench is `--features
        // internals` only, so it's the right place to keep the
        // instrumentation on by default.
        let timing_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("palantir.frame_bench.device"),
                required_features: timing_features,
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .expect("request device");
        Gpu { device, queue }
    })
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

/// Shared scaffolding for every arm: build a fresh `Host`, run 4
/// warmup frames with `PollType::Wait`, then hand criterion the same
/// closure for timing. Each arm's `iter` closure owns target selection
/// and any per-iter state mutation.
fn run_arm<F>(c: &mut Criterion, name: &str, sync: SyncMode, mut iter: F)
where
    F: FnMut(&mut Host, &mut FormState, SyncMode, &wgpu::Device),
{
    let g = gpu();
    let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);
    host.ui.theme.window_clear = Color::BLACK;
    let mut state = FormState::default();
    for _ in 0..4 {
        iter(&mut host, &mut state, SyncMode::Gpu, &g.device);
    }
    c.bench_function(name, |b| {
        b.iter(|| iter(&mut host, &mut state, sync, &g.device));
    });
    // Drain pipelined GPU work before the next bench function reuses
    // the device.
    SyncMode::Gpu.poll(&g.device);
}

fn run_cached(c: &mut Criterion, name: &str, sync: SyncMode) {
    let target = make_target(&gpu().device, CACHED_SIZE, "palantir.frame_bench.cached");
    run_arm(c, name, sync, |host, state, sync, device| {
        host.frame_offscreen(&target, SCALE, |ui| build_ui(state, BENCH_SCALE, ui));
        sync.poll(device);
        black_box(&target);
    });
}

/// Partial-damage arm. Same fixed target as `cached`; the only delta
/// vs. cached is that `state.tick` increments each iter, which mutates
/// the footer "Frame NNNNNNNN" text content. The footer Text is
/// `Sizing::Fixed(120.0)` so the changing digits don't shift siblings —
/// damage resolves to one small rect over an otherwise-static tree,
/// and the renderer hits the `LoadOp::Load + set_scissor_rect` path.
fn run_partial(c: &mut Criterion, name: &str, sync: SyncMode) {
    let target = make_target(&gpu().device, CACHED_SIZE, "palantir.frame_bench.partial");
    assert_partial_invariant(&target);
    run_arm(c, name, sync, |host, state, sync, device| {
        host.frame_offscreen(&target, SCALE, |ui| build_ui(state, BENCH_SCALE, ui));
        state.tick = state.tick.wrapping_add(1);
        sync.poll(device);
        black_box(&target);
    });
}

/// Pin the Partial invariant before the timing loop: spin up a
/// throwaway Host, do a couple of priming frames, then run one frame
/// through the split API so we can inspect `report.plan`. If this
/// ever silently regresses to `Full` (e.g. someone widens the text box
/// and the digits drift the surrounding panel hash), the bench would
/// still produce a number but be measuring the wrong thing.
fn assert_partial_invariant(target: &wgpu::Texture) {
    let g = gpu();
    let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);
    host.ui.theme.window_clear = Color::BLACK;
    let mut state = FormState::default();
    for _ in 0..2 {
        host.frame_offscreen(target, SCALE, |ui| build_ui(&mut state, BENCH_SCALE, ui));
        state.tick = state.tick.wrapping_add(1);
        SyncMode::Gpu.poll(&g.device);
    }
    let display = Display::from_physical(CACHED_SIZE, SCALE);
    let report = host.cpu_frame_for_test(display, |ui| build_ui(&mut state, BENCH_SCALE, ui));
    assert!(
        matches!(report.plan(), Some(RenderPlan::Partial { .. })),
        "frame/partial expected RenderPlan::Partial, got {:?} \
         (fixture's footer-status counter must produce a small damage rect)",
        report.plan(),
    );
}

fn run_resizing(c: &mut Criterion, name: &str, sync: SyncMode) {
    let targets: Vec<wgpu::Texture> = RESIZE_POOL
        .iter()
        .enumerate()
        .map(|(i, s)| {
            make_target(
                &gpu().device,
                *s,
                &format!("palantir.frame_bench.resize.{i}"),
            )
        })
        .collect();
    let mut idx = 0usize;
    run_arm(c, name, sync, |host, state, sync, device| {
        let t = &targets[idx % targets.len()];
        idx = idx.wrapping_add(1);
        host.frame_offscreen(t, SCALE, |ui| build_ui(state, BENCH_SCALE, ui));
        sync.poll(device);
        black_box(t);
    });
}

/// Per-frame `queue.write_*` counts + GPU main-pass time for each
/// arm, frames 0..=5, so the cold→warm transition is visible.
/// Upload columns come from the counting [`palantir::renderer::Queue`]
/// wrapper; the GPU pass column comes from `wgpu` timestamp queries
/// surfaced via [`palantir::renderer::gpu_pass_stats::last_pass_ms`].
/// The pass readout is one frame lagged (the `map_async` callback
/// fires after the next `device.poll`), so frame 0's column is
/// omitted.
fn report_write_stats() {
    fn run(label: &str, targets: &[wgpu::Texture], mut mutate: impl FnMut(&mut FormState, usize)) {
        let g = gpu();
        let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);
        host.ui.theme.window_clear = Color::BLACK;
        let mut state = FormState::default();
        eprintln!("[write_stats] {label}:");
        for frame in 0..6 {
            mutate(&mut state, frame);
            let _ = palantir::renderer::write_stats::take();
            let target = &targets[frame % targets.len()];
            host.frame_offscreen(target, SCALE, |ui| build_ui(&mut state, BENCH_SCALE, ui));
            SyncMode::Gpu.poll(&g.device);
            let s = palantir::renderer::write_stats::take();
            // The pass-time readout lags by one frame (the
            // `map_async` callback that publishes a value fires off
            // the *next* `device.poll`). One extra Poll here drains
            // the just-submitted frame's resolve so the column
            // matches the iteration we're printing rather than the
            // previous one.
            let _ = g.device.poll(wgpu::PollType::Poll);
            let gpu = palantir::renderer::gpu_pass_stats::last_pass_ms()
                .map(|ms| format!("{ms:>5.2} ms"))
                .unwrap_or_else(|| "  n/a   ".into());
            let cc = host.ui.cascade_cache();
            eprintln!(
                "  frame {frame}  buffer: {:>2} calls, {:>9} B   texture: {:>2} calls, {:>9} B   gpu: {gpu}   cache: {:>3} hits / {:>3} misses, {:>3} captures, {:>4} nodes blit",
                s.buffer_calls,
                s.buffer_bytes,
                s.texture_calls,
                s.texture_bytes,
                cc.hits,
                cc.misses,
                cc.captures,
                cc.nodes_blit,
            );
        }
    }

    let g = gpu();
    let cached = [make_target(&g.device, CACHED_SIZE, "write_stats.cached")];
    run("cached", &cached, |_, _| {});

    let partial = [make_target(&g.device, CACHED_SIZE, "write_stats.partial")];
    run("partial", &partial, |state, frame| {
        state.tick = frame as u32;
    });

    let pool: Vec<wgpu::Texture> = RESIZE_POOL
        .iter()
        .enumerate()
        .map(|(i, s)| make_target(&g.device, *s, &format!("write_stats.resize.{i}")))
        .collect();
    run("resizing", &pool, |_, _| {});
}

/// Names of every arm criterion runs, ordered as in `bench_frame`.
/// Used by the per-machine results writer to know which criterion
/// estimate files to read after all arms have finished.
const ARM_NAMES: &[&str] = &[
    "frame/cached_cpu",
    "frame/cached_gpu",
    "frame/partial_cpu",
    "frame/partial_gpu",
    "frame/resizing_cpu",
    "frame/resizing_gpu",
];

fn bench_frame(c: &mut Criterion) {
    report_write_stats();
    run_cached(c, "frame/cached_cpu", SyncMode::Cpu);
    run_cached(c, "frame/cached_gpu", SyncMode::Gpu);
    run_partial(c, "frame/partial_cpu", SyncMode::Cpu);
    run_partial(c, "frame/partial_gpu", SyncMode::Gpu);
    run_resizing(c, "frame/resizing_cpu", SyncMode::Cpu);
    run_resizing(c, "frame/resizing_gpu", SyncMode::Gpu);
    prepend_machine_results();
}

/// Read criterion's `mean` estimate out of `target/criterion/<slug>/new/estimates.json`
/// and write the `[lower mean upper]` triple — same source criterion's
/// stdout prints — to a per-machine `.txt`. Newest run lives at the
/// top of the file (`head` gives the latest). Best-effort: any I/O
/// failure prints to stderr and continues.
fn prepend_machine_results() {
    let machine = machine_label();
    let path = PathBuf::from("benches/results").join(format!("{machine}.txt"));
    let mut block = String::new();
    block.push_str(&format!("=== {} ===\n", now_label()));
    for &name in ARM_NAMES {
        let row = match read_criterion_mean(name) {
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

/// Extract `mean.{lower_bound, point_estimate, upper_bound}` from
/// criterion's `estimates.json`. The file is a single-line JSON blob
/// with a stable layout: `"mean":{"confidence_interval":{...},
/// "point_estimate":N,...}` — slice into the `"mean":` block and pick
/// the three numbers in declaration order. Avoids pulling serde_json
/// just for this.
fn read_criterion_mean(name: &str) -> Option<Estimate> {
    let slug = name.replace('/', "_");
    let path = PathBuf::from("target/criterion")
        .join(&slug)
        .join("new/estimates.json");
    let s = std::fs::read_to_string(&path).ok()?;
    let after_mean = &s[s.find("\"mean\":")? + "\"mean\":".len()..];
    let lo = extract_json_number(after_mean, "\"lower_bound\":")?;
    let hi = extract_json_number(after_mean, "\"upper_bound\":")?;
    let mid = extract_json_number(after_mean, "\"point_estimate\":")?;
    Some(Estimate {
        lo_ns: lo,
        mid_ns: mid,
        hi_ns: hi,
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

/// `PALANTIR_BENCH_MACHINE` overrides the default hostname-derived
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
    if let Ok(env) = std::env::var("PALANTIR_BENCH_MACHINE") {
        let n = sanitize(&env);
        if !n.is_empty() {
            return n;
        }
    }
    let raw = gethostname::gethostname();
    let n = sanitize(&raw.to_string_lossy());
    if n.is_empty() { "unknown".into() } else { n }
}

fn now_label() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%d %H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown-time".into())
}

// Longer per-arm measurement window than criterion's 5 s default —
// the GPU arms (`*_gpu`) bounce ±15-25% on the M5 across back-to-back
// runs because the fanless thermals + scheduler noise share budget
// with everything else on the machine. Doubling the window roughly
// halves the run-to-run spread; total bench wall time goes from ~50 s
// to ~90 s, which is fine for an on-demand bench.
criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(std::time::Duration::from_secs(12))
        .warm_up_time(std::time::Duration::from_secs(3));
    targets = bench_frame
}
criterion_main!(benches);
