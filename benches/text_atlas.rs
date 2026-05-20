//! Text-backend throughput bench: steady state vs. text-scale changes.
//!
//! Motivation: when a graph view zooms, every distinct text scale on
//! the snap-ladder (`TEXT_SCALE_STEP = 0.025` in
//! `renderer/frontend/composer/mod.rs`) mints a fresh per-glyph swash
//! rasterization + atlas slot, which today goes through one
//! `queue.write_texture` per glyph per frame. The visible symptom is a
//! storm of small texture uploads on every frame of a zoom gesture.
//! This bench gives us a baseline to drive a batched-upload
//! optimization.
//!
//! Two cases:
//! - `text_atlas/steady_state` — fixed scale, atlas warm, every glyph
//!   is a `GlyphAtlas::touch` cache hit. Measures the floor cost of
//!   the prepare/encode/render path with text.
//! - `text_atlas/scale_sweep` — scale advances by one ladder step
//!   per frame, cycling through enough rungs that every glyph misses
//!   the atlas each frame (the eviction LRU drops the previous rung
//!   between iterations). This exposes the small-upload cost.
//!
//! Each iteration submits a frame to an offscreen target and waits on
//! `device.poll(Wait)` so queued GPU work drains before the next
//! iteration — without the wait, criterion would just measure CPU
//! submission lag while wgpu buffered work across iterations.
//!
//! Run with:
//!   cargo bench --bench text_atlas --features internals
//!   cargo bench --bench text_atlas --features internals -- 'scale_sweep$'

use std::sync::OnceLock;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use glam::UVec2;
use palantir::{Configure, Host, Panel, Sizing, Text, TextStyle, Ui};
use pollster::FutureExt;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const BASE_SCALE: f32 = 2.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Must exceed `TEXT_SCALE_STEP` (0.025) so each frame lands on a new
/// ladder rung and every glyph misses the atlas. Five steps cycled
/// keeps the cycle short enough that LRU eviction reclaims the slot
/// before we revisit it.
const SCALE_STEP: f32 = 0.04;
const SCALE_CYCLE: u32 = 5;

/// Rows of short labels — graph-view-shaped (many small runs rather
/// than a few wrapped paragraphs). The atlas footprint per scale is
/// dominated by distinct glyphs, not run count, so this is also a
/// realistic miss-storm fixture: ~ASCII letters + digits + a few
/// symbols ≈ a few dozen unique cache keys per scale.
fn build_text_ui(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(4.0)
        .padding(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for row in 0..32 {
                Panel::hstack()
                    .id_salt(("row", row))
                    .gap(12.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Text::new("node")
                            .id_salt(("label", row))
                            .style(TextStyle::default().with_font_size(13.0))
                            .show(ui);
                        Text::new("input: f32")
                            .id_salt(("in", row))
                            .style(TextStyle::default().with_font_size(11.0))
                            .show(ui);
                        Text::new("output: Vec3")
                            .id_salt(("out", row))
                            .style(TextStyle::default().with_font_size(11.0))
                            .show(ui);
                        Text::new("123.45")
                            .id_salt(("val", row))
                            .style(TextStyle::default().with_font_size(11.0))
                            .show(ui);
                    });
            }
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
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .block_on()
            .expect("request adapter (headless)");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("palantir.text_atlas.device"),
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

fn make_target(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.text_atlas.target"),
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
    })
}

fn poll_drain(device: &wgpu::Device) {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("device poll");
}

fn warmed_host(g: &Gpu, target: &wgpu::Texture) -> Host {
    let mut host = Host::new(g.device.clone(), g.queue.clone(), FORMAT);
    // Two warmup frames at the steady scale so the atlas is populated
    // and measure caches latch.
    for _ in 0..2 {
        host.frame_offscreen(target, BASE_SCALE, build_text_ui);
        poll_drain(&g.device);
    }
    host
}

fn bench_text_atlas(c: &mut Criterion) {
    let g = gpu();

    {
        let target = make_target(&g.device);
        let mut host = warmed_host(g, &target);
        let mut group = c.benchmark_group("text_atlas");
        group.measurement_time(Duration::from_secs(5));
        group.bench_function("steady_state", |b| {
            b.iter(|| {
                host.frame_offscreen(&target, BASE_SCALE, build_text_ui);
                poll_drain(&g.device);
            });
        });
        group.finish();
    }

    {
        let target = make_target(&g.device);
        let mut host = warmed_host(g, &target);
        let mut i: u32 = 0;
        let mut group = c.benchmark_group("text_atlas");
        group.measurement_time(Duration::from_secs(5));
        group.bench_function("scale_sweep", |b| {
            b.iter(|| {
                let step = (i % SCALE_CYCLE) as f32;
                let scale = BASE_SCALE + step * SCALE_STEP;
                host.frame_offscreen(&target, scale, build_text_ui);
                poll_drain(&g.device);
                i = i.wrapping_add(1);
            });
        });
        group.finish();
    }
}

criterion_group!(benches, bench_text_atlas);
criterion_main!(benches);
