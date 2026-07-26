//! Per-frame allocation gates for the frame pipeline, driven by `dhat`.
//!
//! Three entry points, each its own `benches/` binary because `dhat`
//! must be the process-wide global allocator:
//!
//! - [`alloc_free`] — strict-zero invariant on aperture's CPU pipeline
//!   (record → measure → arrange → cascade → encode), no GPU.
//! - [`alloc_free_gpu`] — bounded gate on the wgpu submission path,
//!   which allocates a driver-side floor every frame.
//! - [`alloc_resize`] — measurement (not a gate) of the resize path,
//!   where cache busting keeps the allocation surface visible.
//!
//! `dhat` carries 10-30x overhead — never use these binaries for timing.
//! The shared workload is [`FrameFixture`], the same tree
//! [`crate::ui::bench`] times.

use crate::app::App;
use crate::display::Display;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::color::Color;
use crate::ui::Ui;
use crate::ui::bench_fixture::FrameFixture;
use crate::window::WindowToken;
use glam::UVec2;
use pollster::FutureExt;
use std::hint::black_box;
use std::sync::OnceLock;
use std::time::Duration;

// 256 measure frames so an intermittent grow-on-Nth-frame allocation
// (Vec doubling, HashMap rehash) isn't lost between two snapshots.
const MEASURE_FRAMES: usize = 256;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const SCALE: f32 = 2.0;
// Smaller than `ui::bench`'s BENCH_SCALE=32 because the alloc-free
// viewport is 1280x800 instead of 3840x4800 — matches `examples/frame_visual.rs`.
const NODE_SCALE: usize = 6;

/// `DHAT_DUMP=1` swaps the counting-only profiler for the heap profiler
/// that writes `dhat-heap.json` on drop. Every entry point holds the
/// returned guard until after its last measurement, then drops it
/// explicitly — `process::exit` skips `Drop`.
fn profiler() -> dhat::Profiler {
    if std::env::var("DHAT_DUMP").ok().as_deref() == Some("1") {
        dhat::Profiler::new_heap()
    } else {
        dhat::Profiler::builder().testing().build()
    }
}

/// Strict per-frame allocation invariant for aperture's record/measure/
/// arrange/cascade/encode pipeline (no GPU). Pinning test for the
/// `AGENTS.md` claim: "Per-frame allocation is a real metric.
/// Steady-state must be heap-alloc-free after warmup."
///
/// Runs the shared [`FrameFixture`] workload through `Ui::record`, warms
/// up so retained scratch / caches stabilize, then measures heap-block
/// delta over a batch of steady-state frames. **Fails on any non-zero
/// delta** — aperture-side regressions show up here.
///
/// For the GPU submission path (wgpu backend allocations under
/// `WgpuBackend::submit`), see [`alloc_free_gpu`] — driver overhead
/// has a different floor and different semantics.
///
/// Run with: `cargo bench --bench alloc_free --features internals`
/// Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_free --features internals`
pub fn alloc_free() {
    const WARMUP_FRAMES: usize = 16;

    let _profiler = profiler();

    let display = Display::from_physical(PHYSICAL, SCALE);
    // `Ui::default()` (mono-fallback shaper, self-contained); warmed
    // manually below via `WARMUP_FRAMES` before measuring.
    let mut ui = Ui::default();
    let mut state = FrameFixture::default();

    for _ in 0..WARMUP_FRAMES {
        black_box(
            ui.record_test_frame_without_baseline(display, Duration::ZERO, |ui| {
                state.render(NODE_SCALE, ui)
            }),
        );
    }
    let before = dhat::HeapStats::get();
    for _ in 0..MEASURE_FRAMES {
        black_box(
            ui.record_test_frame_without_baseline(display, Duration::ZERO, |ui| {
                state.render(NODE_SCALE, ui)
            }),
        );
    }
    let after = dhat::HeapStats::get();

    let block_delta = after.total_blocks - before.total_blocks;
    let byte_delta = after.total_bytes - before.total_bytes;

    println!(
        "alloc_free: warmup={WARMUP_FRAMES} measure={MEASURE_FRAMES} \
         ({PHYSICAL:?} @ {SCALE}x, node_scale={NODE_SCALE})"
    );
    println!(
        "  record-only           {block_delta:6} blocks  {byte_delta:10} bytes  \
         ({:5.2}/frame, limit strict zero)",
        block_delta as f64 / MEASURE_FRAMES as f64,
    );

    let ok = block_delta == 0 && byte_delta == 0;

    // Drop the profiler explicitly so DHAT_DUMP=1 writes dhat-heap.json
    // before we exit (process::exit skips Drop).
    drop(_profiler);

    if !ok {
        eprintln!();
        eprintln!(
            "FAIL: record-only must be strictly allocation-free; got {:.2} blocks/frame.",
            block_delta as f64 / MEASURE_FRAMES as f64
        );
        eprintln!();
        eprintln!("Inspect call sites with:");
        eprintln!("  DHAT_DUMP=1 cargo bench --bench alloc_free --features internals");
        eprintln!("  open dhat-heap.json at https://nnethercote.github.io/dh_view/");
        std::process::exit(1);
    }

    println!();
    println!("PASS: aperture CPU pipeline is allocation-free in steady state.");
}

// Driver floor on the current wgpu/cosmic-text pin. Bump if a driver
// upgrade or a deliberate aperture change moves the baseline; trip
// the gate otherwise. All current attribution is wgpu_core/wgpu_hal —
// no aperture-side per-frame allocs in this path.
const RENDER_BLOCKS_PER_FRAME_MAX: u64 = 35;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug)]
struct FixtureApp<'a> {
    state: &'a mut FrameFixture,
}

impl App for FixtureApp<'_> {
    fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
        self.state.render(NODE_SCALE, ui);
    }
}

#[derive(Debug)]
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
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .block_on()
            .expect("request adapter (headless)");
        // Text Params via immediates — feature + 16-byte budget.
        let mut limits = wgpu::Limits::default();
        limits.max_immediate_size = limits.max_immediate_size.max(16);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("aperture.alloc_free_gpu.device"),
                required_features: wgpu::Features::IMMEDIATES,
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

/// Per-frame allocation regression gate for the wgpu submission path.
///
/// Sister to [`alloc_free`]. Where that gate asserts a *strict zero* on
/// aperture's CPU pipeline (record → measure → arrange → cascade →
/// encode), this one measures the additional allocations introduced by
/// `OffscreenHost::frame_offscreen` against an offscreen target texture,
/// with a GPU poll between frames so submitted work drains before the
/// next iteration.
///
/// The GPU path is **not** strict zero. Every wgpu submission
/// fundamentally allocates: a `CommandEncoder` Arc, a `CommandBuffer`
/// Arc, the queue's in-flight `Vec` push, plus per-pass scratch from
/// `wgpu_hal::metal`. Current measured floor on this fixture is
/// ~27 blocks/frame, all attributed to wgpu_core/wgpu_hal driver code
/// beneath `OffscreenHost::frame_offscreen` (verified via `DHAT_DUMP=1` +
/// dh_view). The bench treats this as a baseline: the gate trips when
/// the per-frame block count exceeds `RENDER_BLOCKS_PER_FRAME_MAX`,
/// indicating either an aperture regression or a wgpu/cosmic-text
/// version drift worth investigating.
///
/// Run with: `cargo bench --bench alloc_free_gpu --features internals`
/// Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_free_gpu --features internals`
pub fn alloc_free_gpu() {
    const WARMUP_FRAMES: usize = 16;

    let _profiler = profiler();

    let g = gpu();
    // The public offscreen path always copies from its backbuffer so the
    // per-frame alloc floor this bench pins excludes the direct-present path.
    let mut host = OffscreenHost::builder(g.device.clone(), g.queue.clone()).build();
    let mut state = FrameFixture::default();

    let target = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aperture.alloc_free_gpu.target"),
        size: wgpu::Extent3d {
            width: PHYSICAL.x,
            height: PHYSICAL.y,
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
    });
    let run = |host: &mut OffscreenHost, state: &mut FrameFixture| {
        host.ui().theme.window_clear = Color::TRANSPARENT;
        host.frame_offscreen(&target, SCALE, &mut FixtureApp { state })
            .expect("valid benchmark scale factor");
        g.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll");
    };

    for _ in 0..WARMUP_FRAMES {
        run(&mut host, &mut state);
    }
    let before = dhat::HeapStats::get();
    for _ in 0..MEASURE_FRAMES {
        run(&mut host, &mut state);
    }
    let after = dhat::HeapStats::get();

    let block_delta = after.total_blocks - before.total_blocks;
    let byte_delta = after.total_bytes - before.total_bytes;
    let bpf = block_delta as f64 / MEASURE_FRAMES as f64;

    println!(
        "alloc_free_gpu: warmup={WARMUP_FRAMES} measure={MEASURE_FRAMES} \
         ({PHYSICAL:?} @ {SCALE}x, node_scale={NODE_SCALE})"
    );
    println!(
        "  record + render       {block_delta:6} blocks  {byte_delta:10} bytes  \
         ({bpf:5.2}/frame, limit ≤ {RENDER_BLOCKS_PER_FRAME_MAX}/frame)"
    );

    let ok = block_delta <= RENDER_BLOCKS_PER_FRAME_MAX * MEASURE_FRAMES as u64;

    drop(_profiler);

    if !ok {
        eprintln!();
        eprintln!(
            "FAIL: render path exceeds wgpu driver baseline \
             ({bpf:.2} > {RENDER_BLOCKS_PER_FRAME_MAX} blocks/frame)."
        );
        eprintln!();
        eprintln!("Inspect call sites with:");
        eprintln!("  DHAT_DUMP=1 cargo bench --bench alloc_free_gpu --features internals");
        eprintln!("  open dhat-heap.json at https://nnethercote.github.io/dh_view/");
        eprintln!();
        eprintln!(
            "If the baseline legitimately moved (wgpu/cosmic-text upgrade, intentional aperture"
        );
        eprintln!("change), bump RENDER_BLOCKS_PER_FRAME_MAX in src/host/bench.rs.");
        std::process::exit(1);
    }

    println!();
    println!("PASS: render path within wgpu driver baseline.");
}

// Match `ui::bench`'s `BENCH_SCALE` / `RESIZE_POOL` so the resize
// workload is the same shape `frame/resizing_cpu` measures (~800
// nodes, ~500 text shapes).
const RESIZE_NODE_SCALE: usize = 32;

const RESIZE_POOL: &[UVec2] = &[
    UVec2::new(3200, 4400),
    UVec2::new(3840, 4800),
    UVec2::new(3520, 4600),
    UVec2::new(4160, 5000),
];

/// Continuous-drag mode: every frame is a unique width, modelling a
/// user dragging the window edge. With ~256 unique sizes the text /
/// measure / cascade caches never hit on the same width twice, so
/// any per-frame allocation surface stays visible.
fn continuous_size(frame: usize) -> UVec2 {
    let base = UVec2::new(3520, 4600);
    let dx = ((frame * 7) % 800) as i32 - 400;
    UVec2::new((base.x as i32 + dx).max(800) as u32, base.y)
}

/// Steady-state allocation count for the resize path. Mirrors
/// [`alloc_free`] but rotates the `Display` size each frame to bust
/// `MeasureCache` / text-shaping caches the way the
/// `frame/resizing_cpu` arm does. **Not strict-zero** — this measures,
/// it doesn't assert. Use the output to find which call sites are
/// still allocating after warmup.
///
/// Uses `Ui::for_test_text()` (real cosmic-text), NOT `Ui::default()`
/// (mono fallback): the fallback emits a constant paint count across
/// sizes, so the damage `PaintSnapArena` reuses its slots in place
/// and the bench reports a misleading 0 blocks/frame. Real shaping
/// reflows text per size, drifting the paint count and exercising the
/// arena evict/append path the live `frame/resizing_cpu` arm hits.
/// That dependency is why this bench requires the `internals` feature.
///
/// Run with: `cargo bench --bench alloc_resize --features internals`
/// Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_resize --features internals`
pub fn alloc_resize() {
    const WARMUP_FRAMES: usize = 32;

    let _profiler = profiler();

    let mut ui = Ui::for_test_text();
    let mut state = FrameFixture::default();

    // Two arms: pool-rotation (matches `frame/resizing_cpu` exactly)
    // and continuous-drag (every frame a unique width — models a real
    // user dragging the window edge, no cache hits possible).
    let mut run = |label: &str, size: &mut dyn FnMut(usize) -> UVec2| {
        for f in 0..WARMUP_FRAMES {
            let display = Display::from_physical(size(f), SCALE);
            black_box(
                ui.record_test_frame_without_baseline(display, Duration::ZERO, |ui| {
                    state.render(RESIZE_NODE_SCALE, ui)
                }),
            );
        }
        let before = dhat::HeapStats::get();
        for f in 0..MEASURE_FRAMES {
            let display = Display::from_physical(size(f + WARMUP_FRAMES), SCALE);
            black_box(
                ui.record_test_frame_without_baseline(display, Duration::ZERO, |ui| {
                    state.render(RESIZE_NODE_SCALE, ui)
                }),
            );
        }
        let after = dhat::HeapStats::get();

        let block_delta = after.total_blocks - before.total_blocks;
        let byte_delta = after.total_bytes - before.total_bytes;
        println!(
            "  {label:20} {block_delta:6} blocks  {byte_delta:10} bytes  \
             ({:7.2} blocks/frame, {:9.0} bytes/frame)",
            block_delta as f64 / MEASURE_FRAMES as f64,
            byte_delta as f64 / MEASURE_FRAMES as f64,
        );
    };

    println!(
        "alloc_resize: warmup={WARMUP_FRAMES} measure={MEASURE_FRAMES} \
         (node_scale={RESIZE_NODE_SCALE})"
    );

    run("pool-rotation", &mut |f| RESIZE_POOL[f % RESIZE_POOL.len()]);
    run("continuous-drag", &mut continuous_size);

    drop(_profiler);
}
