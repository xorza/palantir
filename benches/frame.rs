//! Per-frame aggregate benchmark — full CPU + GPU.
//!
//! Drives the canonical public API: `Host::frame_offscreen` against an
//! offscreen `wgpu::Texture`, with a blocking GPU poll between
//! iterations so frames don't pipeline and the measured time covers
//! one complete record → measure → arrange → cascade → encode →
//! compose → submit → execute cycle.
//!
//! The `build_ui` workload lives in `benches/support/frame_fixture.rs`
//! and is shared with `examples/frame_visual.rs`, which renders the
//! same scene in a real window for manual visual inspection.
//!
//! Two arms — same workload, different cache state:
//!
//! - **`frame/cached`** — fixed viewport. After criterion's warmup,
//!   `MeasureCache` hits at the highest stable root every frame.
//! - **`frame/resizing`** — rotates through a small pool of
//!   differently-sized offscreen targets, so `available_q` busts on
//!   every iteration and measure rebuilds from scratch. Approximates
//!   a live drag-resize.

#[path = "support/frame_fixture.rs"]
mod fixture;

use criterion::{Criterion, criterion_group, criterion_main};
use fixture::{FormState, build_ui};
use palantir::{Color, Host};
use pollster::FutureExt;
use std::hint::black_box;
use std::sync::OnceLock;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SCALE: f32 = 2.0;
const CACHED_SIZE: glam::UVec2 = glam::UVec2::new(2560, 1600); // 1280x800 @ 2x
const RESIZE_POOL: &[glam::UVec2] = &[
    glam::UVec2::new(2048, 1280),
    glam::UVec2::new(2560, 1600),
    glam::UVec2::new(2304, 1440),
    glam::UVec2::new(2816, 1792),
];

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
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("palantir.frame_bench.device"),
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

fn bench_frame(c: &mut Criterion) {
    let g = gpu();

    // ── Cached arm: one offscreen target, fixed size. Steady-state
    // cost of the full pipeline with warm caches.
    {
        let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);
        host.ui.theme.window_clear = Color::BLACK;
        let target = make_target(&g.device, CACHED_SIZE, "palantir.frame_bench.cached");
        let mut state = FormState::default();
        // Prime caches so the first measured iter is steady-state.
        for _ in 0..4 {
            host.frame_offscreen(&target, SCALE, |ui| build_ui(&mut state, ui));
            g.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .expect("device poll");
        }

        c.bench_function("frame/cached", |b| {
            b.iter(|| {
                host.frame_offscreen(&target, SCALE, |ui| build_ui(&mut state, ui));
                g.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: None,
                    })
                    .expect("device poll");
                black_box(&target);
            });
        });
    }

    // ── Resizing arm: cycle through a pool of differently-sized
    // targets so MeasureCache keys bust each iter. Pool is
    // pre-allocated outside the timing loop.
    {
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
                build_ui(&mut state, ui)
            });
            g.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .expect("device poll");
            idx += 1;
        }

        c.bench_function("frame/resizing", |b| {
            b.iter(|| {
                let target = &targets[idx % targets.len()];
                idx = idx.wrapping_add(1);
                host.frame_offscreen(target, SCALE, |ui| build_ui(&mut state, ui));
                g.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: None,
                    })
                    .expect("device poll");
                black_box(target);
            });
        });
    }
}

criterion_group!(benches, bench_frame);
criterion_main!(benches);
