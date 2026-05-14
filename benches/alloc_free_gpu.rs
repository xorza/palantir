//! Per-frame allocation regression gate for the wgpu submission path.
//!
//! Sister to `alloc_free.rs`. Where that bench asserts a *strict zero*
//! on palantir's CPU pipeline (record → measure → arrange → cascade →
//! encode), this bench measures the additional allocations introduced
//! by `Host::render` against an offscreen target texture, with
//! a GPU poll between frames so submitted work drains before the next
//! iteration.
//!
//! The GPU path is **not** strict zero. Every wgpu submission
//! fundamentally allocates: a `CommandEncoder` Arc, a `CommandBuffer`
//! Arc, the queue's in-flight `Vec` push, plus per-pass scratch from
//! `wgpu_hal::metal`. Current measured floor on this fixture is
//! ~22 blocks/frame, all attributed to wgpu_core/wgpu_hal driver code
//! beneath `Host::render` (verified via `DHAT_DUMP=1` +
//! dh_view). The bench treats this as a baseline: the gate trips when
//! the per-frame block count exceeds `RENDER_BLOCKS_PER_FRAME_MAX`,
//! indicating either a palantir regression or a wgpu/glyphon version
//! drift worth investigating.
//!
//! Uses `dhat` as the global allocator (10-30x overhead — never use
//! this binary for timing).
//!
//! Run with: `cargo bench --bench alloc_free_gpu`
//! Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_free_gpu`

use std::sync::OnceLock;

use glam::UVec2;
use palantir::{
    Align, Button, Color, Configure, Frame, Host, Justify, Panel, Sizing, Text, TextStyle, Ui,
};
use pollster::FutureExt;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const WARMUP_FRAMES: usize = 16;
const MEASURE_FRAMES: usize = 256;

// Driver floor on the current wgpu/glyphon pin. Bump if a driver
// upgrade or a deliberate palantir change moves the baseline; trip
// the gate otherwise. All current attribution is wgpu_core/wgpu_hal —
// no palantir-side per-frame allocs in this path.
const RENDER_BLOCKS_PER_FRAME_MAX: u64 = 35;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const SCALE: f32 = 2.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn build_ui(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(8.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::hstack()
                .auto_id()
                .gap(8.0)
                .size((Sizing::FILL, Sizing::Hug))
                .child_align(Align::CENTER)
                .show(ui, |ui| {
                    Text::new("Alloc-free pinning fixture")
                        .id_salt("title")
                        .style(TextStyle::default().with_font_size(18.0))
                        .show(ui);
                    Frame::new()
                        .id_salt("title-spacer")
                        .size((Sizing::FILL, Sizing::Fixed(1.0)))
                        .show(ui);
                    for i in 0..3 {
                        Button::new().id_salt(("act", i)).label("Action").show(ui);
                    }
                });

            for i in 0..32 {
                Panel::hstack()
                    .id_salt(("row", i))
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt(("avatar", i))
                            .size((Sizing::Fixed(28.0), Sizing::Fixed(28.0)))
                            .show(ui);
                        Panel::vstack()
                            .id_salt(("col", i))
                            .gap(2.0)
                            .size((Sizing::FILL, Sizing::Hug))
                            .show(ui, |ui| {
                                Text::new("name")
                                    .id_salt(("name", i))
                                    .style(TextStyle::default().with_font_size(12.0))
                                    .show(ui);
                                Text::new(
                                    "longer message body that should wrap inside the Fill column",
                                )
                                .id_salt(("body", i))
                                .style(TextStyle::default().with_font_size(13.0))
                                .wrapping()
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui);
                            });
                    });
            }

            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::Fixed(28.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("footer-bg")
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui);
                    Panel::hstack()
                        .auto_id()
                        .padding(4.0)
                        .justify(Justify::Center)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Text::new("Ready")
                                .id_salt("status")
                                .style(TextStyle::default().with_font_size(11.0))
                                .show(ui);
                        });
                });
        });
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
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .block_on()
            .expect("request adapter (headless)");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("palantir.alloc_free_gpu.device"),
                required_features: wgpu::Features::empty(),
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

fn main() {
    let want_dump = std::env::var("DHAT_DUMP").ok().as_deref() == Some("1");
    let _profiler = if want_dump {
        Some(dhat::Profiler::new_heap())
    } else {
        Some(dhat::Profiler::builder().testing().build())
    };

    let g = gpu();
    let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);

    let target = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.alloc_free_gpu.target"),
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
    let run = |host: &mut Host| {
        host.ui.theme.window_clear = Color::TRANSPARENT;
        host.frame_offscreen(&target, SCALE, &mut (), build_ui);
        g.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll");
    };

    for _ in 0..WARMUP_FRAMES {
        run(&mut host);
    }
    let before = dhat::HeapStats::get();
    for _ in 0..MEASURE_FRAMES {
        run(&mut host);
    }
    let after = dhat::HeapStats::get();

    let block_delta = after.total_blocks - before.total_blocks;
    let byte_delta = after.total_bytes - before.total_bytes;
    let bpf = block_delta as f64 / MEASURE_FRAMES as f64;

    println!(
        "alloc_free_gpu: warmup={WARMUP_FRAMES} measure={MEASURE_FRAMES} \
         ({PHYSICAL:?} @ {SCALE}x)"
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
        eprintln!("  DHAT_DUMP=1 cargo bench --bench alloc_free_gpu");
        eprintln!("  open dhat-heap.json at https://nnethercote.github.io/dh_view/");
        eprintln!();
        eprintln!("If the baseline legitimately moved (wgpu/glyphon upgrade, intentional palantir");
        eprintln!("change), bump RENDER_BLOCKS_PER_FRAME_MAX in benches/alloc_free_gpu.rs.");
        std::process::exit(1);
    }

    println!();
    println!("PASS: render path within wgpu driver baseline.");
}
