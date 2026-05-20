//! Text-backend microbench: prepare + flush + render directly against
//! `TextBackend`, bypassing the full `Host` pipeline.
//!
//! The previous version drove `Host::frame_offscreen`, which mixed
//! record/measure/cascade/encode noise into every sample —
//! `CascadesEngine::run` was the top hotspot at ~7%, and the actual
//! text path (`encode_batch` + atlas uploads) totalled <10%. This
//! bench skips all of that: a fixed slice of `TextRun`s, shaped once
//! at construction, fed into `TextBackend::prepare` →
//! `flush_atlas_uploads` → `render_batch` each iteration.
//!
//! Two motivating workloads:
//!
//! - `text_atlas/steady_warm` — fixed scale, atlas warmed by two
//!   priming iterations. Every glyph is an `atlas.touch` hit; the
//!   measurement floor is `encode_batch` walking layout runs +
//!   `swash_cache::CacheKey::new` + vertex buffer upload + draw.
//! - `text_atlas/zoom_smooth` — scale advances by `TEXT_SCALE_STEP`
//!   (0.025) each frame. Matches a real zoom gesture: each rung is a
//!   fresh cosmic `CacheKey` (font_size × scale) → fresh swash
//!   rasterization → fresh atlas slot → `queue.write_texture` per
//!   glyph. Cycles through `SCALE_CYCLE` rungs so the LRU eventually
//!   evicts old rungs.
//! - `text_atlas/zoom_cold` — scale jumps `5 × TEXT_SCALE_STEP` each
//!   frame across `SCALE_CYCLE` rungs. Worst-case miss-storm without
//!   running off the ladder entirely.
//!
//! Each iteration:
//!   1. begin command encoder
//!   2. `prepare` (shape lookup + encode_batch into instance Vec +
//!      potential atlas grow + vbuf upload + params reupload)
//!   3. `flush_atlas_uploads` (drain pending glyph uploads into
//!      encoder)
//!   4. render pass: `render_batch` → submit → `poll(Wait)` so the
//!      GPU work drains before the next iteration.
//!   5. `end_frame` (atlas trim + clear instance Vec + reset ranges)
//!
//! Run with:
//!   cargo bench --bench text_atlas --features internals
//!   cargo bench --bench text_atlas --features internals -- 'zoom_smooth$'

use std::sync::OnceLock;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use glam::{UVec2, Vec2};
use palantir::ColorU8;
use palantir::TextShaper;
use palantir::text_backend::test_support::{GpuCtx, TextBackend, TextRun, make_run};
use pollster::FutureExt;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BASE_SCALE: f32 = 2.0;
const TEXT_SCALE_STEP: f32 = 0.025;
const SCALE_CYCLE: u32 = 5;

/// Per-frame text count. Graph-view-shaped: many small runs rather
/// than a few wrapped paragraphs. 32 rows × 4 columns = 128 runs ≈
/// what the showcase's node graph tab paints.
const ROWS: u32 = 32;

struct Gpu {
    device: wgpu::Device,
    queue: palantir::renderer::Queue,
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
        Gpu {
            device,
            queue: palantir::renderer::Queue::new(queue),
        }
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

/// Shape one frame's worth of runs against `shaper`. Stable layout so
/// the same `TextRun` slice is reusable across iterations; only the
/// per-iteration `scale` argument to `prepare` changes between frames.
fn build_runs(shaper: &TextShaper) -> Vec<TextRun> {
    let color = ColorU8::rgba(220, 220, 220, 255);
    let mut runs = Vec::with_capacity((ROWS * 4) as usize);
    for row in 0..ROWS {
        let y = 16.0 + (row as f32) * 18.0;
        // Four short labels per row at typical graph-node sizes.
        let label_color = ColorU8::rgba(245, 245, 245, 255);
        runs.push(make_run(
            shaper,
            "node",
            13.0,
            13.0 * 1.2,
            Vec2::new(16.0, y),
            PHYSICAL,
            1.0,
            label_color,
        ));
        runs.push(make_run(
            shaper,
            "input: f32",
            11.0,
            11.0 * 1.2,
            Vec2::new(80.0, y),
            PHYSICAL,
            1.0,
            color,
        ));
        runs.push(make_run(
            shaper,
            "output: Vec3",
            11.0,
            11.0 * 1.2,
            Vec2::new(220.0, y),
            PHYSICAL,
            1.0,
            color,
        ));
        runs.push(make_run(
            shaper,
            "123.45",
            11.0,
            11.0 * 1.2,
            Vec2::new(380.0, y),
            PHYSICAL,
            1.0,
            color,
        ));
    }
    runs
}

/// One iteration: prepare → flush → render pass → submit → poll →
/// post. Mirrors `Host::frame_offscreen`'s text-relevant slice.
fn run_frame(
    g: &Gpu,
    backend: &mut TextBackend,
    belt: &mut wgpu::util::StagingBelt,
    target_view: &wgpu::TextureView,
    runs: &[TextRun],
    scale: f32,
) {
    let mut encoder = g
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("palantir.text_atlas.encoder"),
        });
    {
        let mut ctx = GpuCtx::new(&g.device, &g.queue, belt, &mut encoder);
        backend.prepare(&mut ctx, scale, runs);
        backend.flush(&mut ctx);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.text_atlas.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        backend.draw(&mut pass);
    }
    belt.finish();
    g.queue.submit([encoder.finish()]);
    belt.recall();
    poll_drain(&g.device);
    backend.end_frame();
}

fn fresh_backend(g: &Gpu) -> (TextBackend, Vec<TextRun>) {
    let shaper = TextShaper::with_bundled_fonts();
    let runs = build_runs(&shaper);
    let mut backend = TextBackend::new_for_bench(&g.device, FORMAT, shaper);
    backend.set_viewport(PHYSICAL);
    (backend, runs)
}

fn bench_text_atlas(c: &mut Criterion) {
    let g = gpu();
    let target = make_target(&g.device);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let mut group = c.benchmark_group("text_atlas");
    group.measurement_time(Duration::from_secs(5));

    {
        let (mut backend, runs) = fresh_backend(g);
        let mut belt = wgpu::util::StagingBelt::new(g.device.clone(), 1 << 20);
        // Two priming frames so every glyph is in the atlas.
        for _ in 0..2 {
            run_frame(g, &mut backend, &mut belt, &view, &runs, BASE_SCALE);
        }
        group.bench_function("steady_warm", |b| {
            b.iter(|| {
                run_frame(g, &mut backend, &mut belt, &view, &runs, BASE_SCALE);
            });
        });
        // CPU-only: prepare + end_frame, no encoder/submit/poll.
        // Isolates text-backend CPU work from GPU sync — useful when
        // the full case looks GPU-bound and you want to see whether a
        // change moved the CPU prepare cost. Still needs a belt +
        // throwaway encoder to satisfy `prepare`'s signature; the
        // encoder is discarded.
        group.bench_function("steady_warm_cpu", |b| {
            b.iter(|| {
                let mut encoder =
                    g.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("palantir.text_atlas.cpu_prepare"),
                        });
                {
                    let mut ctx = GpuCtx::new(&g.device, &g.queue, &mut belt, &mut encoder);
                    backend.prepare(&mut ctx, BASE_SCALE, &runs);
                }
                belt.finish();
                belt.recall();
                backend.end_frame();
            });
        });
    }

    {
        let (mut backend, runs) = fresh_backend(g);
        let mut belt = wgpu::util::StagingBelt::new(g.device.clone(), 1 << 20);
        // Prime the cycle so the LRU has all rungs resident before the
        // measured loop starts evicting + re-inserting.
        for step in 0..SCALE_CYCLE {
            let scale = BASE_SCALE + (step as f32) * TEXT_SCALE_STEP;
            run_frame(g, &mut backend, &mut belt, &view, &runs, scale);
        }
        let mut i: u32 = 0;
        group.bench_function("zoom_smooth", |b| {
            b.iter(|| {
                let step = (i % SCALE_CYCLE) as f32;
                let scale = BASE_SCALE + step * TEXT_SCALE_STEP;
                run_frame(g, &mut backend, &mut belt, &view, &runs, scale);
                i = i.wrapping_add(1);
            });
        });
    }

    {
        let (mut backend, runs) = fresh_backend(g);
        let mut belt = wgpu::util::StagingBelt::new(g.device.clone(), 1 << 20);
        let stride = 5.0 * TEXT_SCALE_STEP;
        for step in 0..SCALE_CYCLE {
            let scale = BASE_SCALE + (step as f32) * stride;
            run_frame(g, &mut backend, &mut belt, &view, &runs, scale);
        }
        let mut i: u32 = 0;
        group.bench_function("zoom_cold", |b| {
            b.iter(|| {
                let step = (i % SCALE_CYCLE) as f32;
                let scale = BASE_SCALE + step * stride;
                run_frame(g, &mut backend, &mut belt, &view, &runs, scale);
                i = i.wrapping_add(1);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_text_atlas);
criterion_main!(benches);
