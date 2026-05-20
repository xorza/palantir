//! Per-frame aggregate benchmark — CPU + GPU.
//!
//! Drives the canonical public API: `Host::frame_offscreen` against an
//! offscreen `wgpu::Texture`. Two arms × two sync modes:
//!
//! - **`frame/cached_*`** — fixed viewport, MeasureCache hits.
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
//! The `build_ui` workload lives in `benches/support/frame_fixture.rs`
//! and is shared with `examples/frame_visual.rs`.

#[path = "support/frame_fixture.rs"]
mod fixture;

use criterion::{Criterion, criterion_group, criterion_main};
use fixture::{BENCH_SCALE, FormState, build_ui};
use palantir::{Color, Host};
use pollster::FutureExt;
use std::hint::black_box;
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

fn run_cached(c: &mut Criterion, name: &str, sync: SyncMode) {
    let g = gpu();
    let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);
    host.ui.theme.window_clear = Color::BLACK;
    let target = make_target(&g.device, CACHED_SIZE, "palantir.frame_bench.cached");
    let mut state = FormState::default();
    // Warmup with Wait to drain pre-bench setup work regardless of mode.
    for _ in 0..4 {
        host.frame_offscreen(&target, SCALE, |ui| build_ui(&mut state, BENCH_SCALE, ui));
        SyncMode::Gpu.poll(&g.device);
    }
    c.bench_function(name, |b| {
        b.iter(|| {
            host.frame_offscreen(&target, SCALE, |ui| build_ui(&mut state, BENCH_SCALE, ui));
            sync.poll(&g.device);
            black_box(&target);
        });
    });
    // Drain pipelined GPU work before the next bench function reuses
    // the device.
    SyncMode::Gpu.poll(&g.device);
}

fn run_resizing(c: &mut Criterion, name: &str, sync: SyncMode) {
    let g = gpu();
    let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);
    host.ui.theme.window_clear = Color::BLACK;
    let targets: Vec<wgpu::Texture> = RESIZE_POOL
        .iter()
        .enumerate()
        .map(|(i, s)| make_target(&g.device, *s, &format!("palantir.frame_bench.resize.{i}")))
        .collect();
    let mut state = FormState::default();
    let mut idx = 0usize;
    for _ in 0..4 {
        host.frame_offscreen(&targets[idx % targets.len()], SCALE, |ui| {
            build_ui(&mut state, BENCH_SCALE, ui)
        });
        SyncMode::Gpu.poll(&g.device);
        idx += 1;
    }
    c.bench_function(name, |b| {
        b.iter(|| {
            let target = &targets[idx % targets.len()];
            idx = idx.wrapping_add(1);
            host.frame_offscreen(target, SCALE, |ui| build_ui(&mut state, BENCH_SCALE, ui));
            sync.poll(&g.device);
            black_box(target);
        });
    });
    SyncMode::Gpu.poll(&g.device);
}

/// Per-frame `queue.write_*` counts + GPU main-pass time for both
/// arms, frames 0..=5, so the cold→warm transition is visible.
/// Upload columns come from the counting [`Queue`] wrapper; the GPU
/// pass column comes from `wgpu` timestamp queries surfaced via
/// [`palantir::renderer::gpu_pass_stats::last_pass_ms`]. The pass
/// readout is one frame lagged (the `map_async` callback fires
/// after the next `device.poll`), so frame 0's column is omitted.
fn report_write_stats() {
    fn run<F: FnMut(usize) -> &'static wgpu::Texture>(label: &str, mut pick: F) {
        let g = gpu();
        let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);
        host.ui.theme.window_clear = Color::BLACK;
        let mut state = FormState::default();
        eprintln!("[write_stats] {label}:");
        for frame in 0..6 {
            let _ = palantir::renderer::write_stats::take();
            host.frame_offscreen(pick(frame), SCALE, |ui| {
                build_ui(&mut state, BENCH_SCALE, ui)
            });
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
            eprintln!(
                "  frame {frame}  buffer: {:>2} calls, {:>9} B   texture: {:>2} calls, {:>9} B   gpu: {gpu}",
                s.buffer_calls, s.buffer_bytes, s.texture_calls, s.texture_bytes,
            );
        }
    }

    let g = gpu();
    let cached: &'static wgpu::Texture = Box::leak(Box::new(make_target(
        &g.device,
        CACHED_SIZE,
        "write_stats.cached",
    )));
    run("cached", |_| cached);

    let pool: &'static [wgpu::Texture] = Box::leak(
        RESIZE_POOL
            .iter()
            .enumerate()
            .map(|(i, s)| make_target(&g.device, *s, &format!("write_stats.resize.{i}")))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    run("resizing", move |frame| &pool[frame % pool.len()]);
}

fn bench_frame(c: &mut Criterion) {
    report_write_stats();
    run_cached(c, "frame/cached_cpu", SyncMode::Cpu);
    run_cached(c, "frame/cached_gpu", SyncMode::Gpu);
    run_resizing(c, "frame/resizing_cpu", SyncMode::Cpu);
    run_resizing(c, "frame/resizing_gpu", SyncMode::Gpu);
}

criterion_group!(benches, bench_frame);
criterion_main!(benches);
