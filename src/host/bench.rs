//! Per-frame allocation gates for the frame pipeline, driven by `dhat`.
//!
//! One bench — [`alloc`] — of four steps, each warming up and then
//! counting what a batch of steady-state frames allocates:
//!
//! | step | measures | limit |
//! |---|---|---|
//! | `record-only` | palantir's CPU pipeline: record → measure → arrange → cascade → encode, no GPU | strict zero |
//! | `record + render` | the same plus `OffscreenHost::frame_offscreen`, i.e. the wgpu submission path | the driver floor |
//! | `resize pool-rotation` | the resize path over [`RESIZE_POOL`], the shape `frame/resizing_cpu` measures | reported |
//! | `resize drag` | resize with a unique width per frame, so no cache ever hits twice | reported |
//!
//! Steps rather than separate binaries because they share one profiler
//! and answer one question — *does a steady-state frame allocate?* — so
//! running one without the others tells you less than the four together.
//! Every step reports; the run fails if any bounded one is over.
//!
//! It is its own target only because `dhat::Alloc` must be the
//! process-wide global allocator, which would tax every criterion timing
//! sharing the binary 10-30x. For the same reason these numbers are
//! allocation counts, never times. The workload is [`FrameFixture`], the
//! same tree [`crate::ui::bench`] times.

use crate::app::App;
use crate::frame_fixture::FrameFixture;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::color::Color;
use crate::ui::Ui;
use crate::ui::harness::UiHarness;
use crate::window::WindowToken;
use glam::UVec2;
use pollster::FutureExt;
use std::hint::black_box;
use std::sync::OnceLock;

// 256 measure frames so an intermittent grow-on-Nth-frame allocation
// (Vec doubling, HashMap rehash) isn't lost between two snapshots.
const MEASURE_FRAMES: usize = 256;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const SCALE: f32 = 2.0;
// Smaller than `ui::bench`'s BENCH_SCALE=32 because the alloc-free
// viewport is 1280x800 instead of 3840x4800 — matches the showcase's
// `frame bench` page.
const NODE_SCALE: usize = 6;

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
    /// Not one block, not one byte. palantir's own code, where the
    /// posture is an invariant rather than a budget.
    Zero,
    /// At most this many blocks per frame — a floor owned by someone
    /// else (the wgpu driver), so the gate catches drift, not presence.
    BlocksPerFrame(u64),
    /// Reported, never enforced. Cache-busting paths allocate by
    /// design; the number is there to be watched, not to gate.
    Reported,
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
    /// what `MEASURE_FRAMES` of steady state allocate. `frame` gets the
    /// absolute frame index so a step can vary the surface per frame
    /// without the warmup and the measured window overlapping sizes.
    fn measure(
        name: &'static str,
        limit: Limit,
        warmup: usize,
        mut frame: impl FnMut(usize),
    ) -> Step {
        for f in 0..warmup {
            frame(f);
        }
        let before = dhat::HeapStats::get();
        for f in 0..MEASURE_FRAMES {
            frame(warmup + f);
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

    fn over_limit(&self) -> bool {
        match self.limit {
            Limit::Zero => self.blocks != 0 || self.bytes != 0,
            Limit::BlocksPerFrame(max) => self.blocks > max * MEASURE_FRAMES as u64,
            Limit::Reported => false,
        }
    }

    fn report(&self) {
        let limit = match self.limit {
            Limit::Zero => "limit strict zero".to_owned(),
            Limit::BlocksPerFrame(max) => format!("limit <= {max}/frame"),
            Limit::Reported => "measured, not gated".to_owned(),
        };
        println!(
            "  {:<22} {:6} blocks  {:10} bytes  ({:7.2}/frame, {limit})",
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
fn record_only() -> Step {
    // `Ui::new` over isolated mono resources; warmed by `Step::measure`
    // before it starts counting.
    let mut h = UiHarness::new(PHYSICAL).scale(SCALE);
    let mut state = FrameFixture::default();
    Step::measure("record-only", Limit::Zero, 16, |_| {
        black_box(h.frame(|ui| state.render(NODE_SCALE, ui)));
    })
}

// Driver floor on the current wgpu/cosmic-text pin. Bump if a driver
// upgrade or a deliberate palantir change moves the baseline; trip
// the gate otherwise. All current attribution is wgpu_core/wgpu_hal —
// no palantir-side per-frame allocs in this path.
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
                label: Some("palantir.alloc.device"),
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

/// Step 2 — the same frame plus `OffscreenHost::frame_offscreen`
/// against an offscreen target, with a poll between frames so submitted
/// work drains before the next.
///
/// **Not** strict zero, and cannot be: every wgpu submission allocates a
/// `CommandEncoder` Arc, a `CommandBuffer` Arc, the queue's in-flight
/// `Vec` push, and per-pass scratch from `wgpu_hal`. The measured floor
/// on this fixture is ~27 blocks/frame, all attributed to
/// wgpu_core/wgpu_hal beneath `frame_offscreen` (verified in dh_view via
/// `--dump`). So the gate catches *drift* from that floor: a palantir
/// regression, or a wgpu/cosmic-text bump worth looking at.
fn record_and_render() -> Step {
    let g = gpu();
    // The public offscreen path always copies from its backbuffer, so
    // the floor pinned here excludes the direct-present path.
    let mut host = OffscreenHost::builder(g.device.clone(), g.queue.clone()).build();
    let mut state = FrameFixture::default();

    let target = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.alloc.render.target"),
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

    Step::measure(
        "record + render",
        Limit::BlocksPerFrame(RENDER_BLOCKS_PER_FRAME_MAX),
        16,
        |_| {
            host.ui().theme.window_clear = Color::TRANSPARENT;
            host.frame_offscreen(&target, SCALE, &mut FixtureApp { state: &mut state });
            g.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .expect("device poll");
        },
    )
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

/// Steps 3 and 4 — the resize path, rotating the `Display` size to bust
/// `MeasureCache` and the text-shaping caches the way `frame/resizing_cpu`
/// does. Reported rather than gated: cache-busting allocates by design,
/// and the number exists to show which call sites still do after warmup.
///
/// `pool-rotation` matches `frame/resizing_cpu` exactly; `drag` gives
/// every frame a unique width, modelling a user dragging the window edge
/// so no cache can hit the same width twice.
///
/// Both use `UiHarness::with_text` (real cosmic-text), **not** the mono
/// fallback: the fallback emits a constant paint count across sizes, so
/// the damage `PaintSnapArena` reuses its slots in place and the step
/// reports a misleading 0 blocks/frame. Real shaping reflows text per
/// size, drifting the paint count and exercising the arena evict/append
/// path the live arm hits. That is why this bench needs `internals`.
fn resize(name: &'static str, mut size: impl FnMut(usize) -> UVec2) -> Step {
    let mut h = UiHarness::with_text(PHYSICAL).scale(SCALE);
    let mut state = FrameFixture::default();
    Step::measure(name, Limit::Reported, 32, |f| {
        black_box(
            h.resize(size(f))
                .frame(|ui| state.render(RESIZE_NODE_SCALE, ui)),
        );
    })
}

/// The allocation bench: every step, one profiler, one verdict.
///
/// Steps run to completion even when an earlier one is over its limit —
/// four numbers localize a regression, one plus an early exit does not.
pub(crate) fn alloc(dump: bool) {
    let profiler = profiler(dump);

    println!(
        "alloc: measure={MEASURE_FRAMES} frames/step \
         ({PHYSICAL:?} @ {SCALE}x, node_scale={NODE_SCALE}/{RESIZE_NODE_SCALE})"
    );
    let steps = [
        record_only(),
        record_and_render(),
        resize("resize pool-rotation", |f| {
            RESIZE_POOL[f % RESIZE_POOL.len()]
        }),
        resize("resize drag", continuous_size),
    ];
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
            Limit::Reported => unreachable!("a reported step is never over"),
        }
    }
    eprintln!();
    eprintln!("Inspect call sites with:");
    eprintln!("  cargo bench --bench alloc --features bench -- --dump");
    eprintln!("  open dhat-heap.json at https://nnethercote.github.io/dh_view/");
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
