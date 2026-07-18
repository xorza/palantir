//! Per-frame allocation regression gate for the wgpu submission path.
//!
//! Sister to `alloc_free.rs`. Where that bench asserts a *strict zero*
//! on aperture's CPU pipeline (record → measure → arrange → cascade →
//! encode), this bench measures the additional allocations introduced
//! by `WindowDriver::frame_offscreen` against an offscreen target texture, with
//! a GPU poll between frames so submitted work drains before the next
//! iteration.
//!
//! The GPU path is **not** strict zero. Every wgpu submission
//! fundamentally allocates: a `CommandEncoder` Arc, a `CommandBuffer`
//! Arc, the queue's in-flight `Vec` push, plus per-pass scratch from
//! `wgpu_hal::metal`. Current measured floor on this fixture is
//! ~27 blocks/frame, all attributed to wgpu_core/wgpu_hal driver code
//! beneath `WindowDriver::frame_offscreen` (verified via `DHAT_DUMP=1` +
//! dh_view). The bench treats this as a baseline: the gate trips when
//! the per-frame block count exceeds `RENDER_BLOCKS_PER_FRAME_MAX`,
//! indicating either an aperture regression or a wgpu/cosmic-text
//! version drift worth investigating.
//!
//! Uses `dhat` as the global allocator (10-30x overhead — never use
//! this binary for timing).
//!
//! Run with: `cargo bench --bench alloc_free_gpu --features internals`
//! Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_free_gpu --features internals`

use std::sync::OnceLock;

use aperture::{App, Color, OffscreenHost, Ui, WindowToken, bench::FrameFixture};
use glam::UVec2;
use pollster::FutureExt;

const WARMUP_FRAMES: usize = 16;
const MEASURE_FRAMES: usize = 256;

// Driver floor on the current wgpu/cosmic-text pin. Bump if a driver
// upgrade or a deliberate aperture change moves the baseline; trip
// the gate otherwise. All current attribution is wgpu_core/wgpu_hal —
// no aperture-side per-frame allocs in this path.
const RENDER_BLOCKS_PER_FRAME_MAX: u64 = 35;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const SCALE: f32 = 2.0;
// Smaller than `frame.rs`'s BENCH_SCALE=32 because the alloc-free
// viewport is 1280x800 instead of 3840x4800 — matches `examples/frame_visual.rs`.
const NODE_SCALE: usize = 6;
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

pub fn bench() {
    let want_dump = std::env::var("DHAT_DUMP").ok().as_deref() == Some("1");
    let _profiler = if want_dump {
        Some(dhat::Profiler::new_heap())
    } else {
        Some(dhat::Profiler::builder().testing().build())
    };

    let g = gpu();
    // The public offscreen path always copies from its backbuffer so the
    // per-frame alloc floor this bench pins excludes the direct-present path.
    let mut host = OffscreenHost::builder(
        WindowToken(0),
        g.device.clone(),
        g.queue.clone(),
        aperture::TextShaper::with_bundled_fonts(),
    )
    .build();
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
        host.frame_offscreen(&target, SCALE, &mut FixtureApp { state });
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
        eprintln!("change), bump RENDER_BLOCKS_PER_FRAME_MAX in src/bench/allocation/free_gpu.rs.");
        std::process::exit(1);
    }

    println!();
    println!("PASS: render path within wgpu driver baseline.");
}
