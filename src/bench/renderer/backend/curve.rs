//! GPU curve-pipeline benchmark. Two fixed workloads isolate the shader paths
//! affected by the static strip index buffer:
//!
//! - `cubic_strips` records one short cubic per grid cell. Every cubic stays
//!   below the composer's subdivision threshold and produces one instance.
//! - `join_chrome` records one three-point polyline per grid cell. Each emits
//!   two segment instances and one join-chrome instance.
//!
//! Each iteration toggles one control point so damage forces the full curve
//! stream through the backend, then waits for the GPU. Criterion measures that
//! complete record-to-GPU wall time; the keep-or-revert signal is the median
//! curve timestamp and pipeline statistics printed before each case.

use crate::app::test_support::RecordApp;
use crate::diagnostics::gpu_stats::BatchKind;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::color::Color;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::LineJoin;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::window::WindowToken;
use crate::{Configure, Sizing, Vec2};
use criterion::{Criterion, Throughput};
use pollster::FutureExt;
use std::hint::black_box;
use std::sync::OnceLock;
use std::time::Duration;

const PHYSICAL: glam::UVec2 = glam::UVec2::new(1024, 1024);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const GRID: u32 = 64;
const CELL: f32 = 16.0;
const CUBIC_INSTANCES: u64 = (GRID * GRID) as u64;
const JOIN_INSTANCES: u64 = (GRID * GRID * 3) as u64;
const EVIDENCE_FRAMES: usize = 64;

#[derive(Clone, Copy, Debug)]
enum Workload {
    CubicStrips,
    JoinChrome,
}

impl Workload {
    const fn label(self) -> &'static str {
        match self {
            Self::CubicStrips => "cubic_strips",
            Self::JoinChrome => "join_chrome",
        }
    }

    const fn instances(self) -> u64 {
        match self {
            Self::CubicStrips => CUBIC_INSTANCES,
            Self::JoinChrome => JOIN_INSTANCES,
        }
    }
}

#[derive(Debug)]
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    timing_features: wgpu::Features,
}

fn gpu() -> &'static Gpu {
    static GPU: OnceLock<Gpu> = OnceLock::new();
    GPU.get_or_init(|| {
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
        let timing_features = adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
                | wgpu::Features::PIPELINE_STATISTICS_QUERY);
        let mut limits = wgpu::Limits::default();
        limits.max_immediate_size = limits.max_immediate_size.max(16);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("aperture.curve_pipeline_bench.device"),
                required_features: timing_features | wgpu::Features::IMMEDIATES,
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .expect("request device");
        Gpu {
            device,
            queue,
            info: adapter.get_info(),
            timing_features,
        }
    })
}

fn target(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aperture.curve_pipeline_bench.target"),
        size: wgpu::Extent3d {
            width: PHYSICAL.x,
            height: PHYSICAL.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn host(gpu: &Gpu) -> OffscreenHost {
    let mut host = OffscreenHost::builder(
        WindowToken(0),
        gpu.device.clone(),
        gpu.queue.clone(),
        TextShaper::with_bundled_fonts(),
    )
    .collect_gpu_stats(true)
    .build();
    host.ui().theme.panel_background = None;
    host
}

fn poll(device: &wgpu::Device) {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("device poll");
}

fn record(ui: &mut Ui, workload: Workload, phase: bool) {
    Panel::zstack()
        .id_salt("curve-pipeline-bench-root")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| match workload {
            Workload::CubicStrips => record_cubics(ui, phase),
            Workload::JoinChrome => record_joins(ui, phase),
        });
}

fn record_cubics(ui: &mut Ui, phase: bool) {
    let color = Color::rgb(0.2, 0.8, 1.0);
    let wobble = if phase { 0.125 } else { -0.125 };
    for row in 0..GRID {
        for col in 0..GRID {
            let origin = Vec2::new(col as f32 * CELL, row as f32 * CELL);
            ui.add_shape(
                Shape::cubic_bezier(
                    origin + Vec2::new(2.0, 8.0),
                    origin + Vec2::new(5.0, 5.5 + wobble),
                    origin + Vec2::new(11.0, 10.5),
                    origin + Vec2::new(14.0, 8.0),
                    2.0,
                )
                .brush(color),
            );
        }
    }
}

fn record_joins(ui: &mut Ui, phase: bool) {
    let color = Color::rgba(0.3, 1.0, 0.5, 0.75);
    let wobble = if phase { 0.125 } else { -0.125 };
    for row in 0..GRID {
        for col in 0..GRID {
            let origin = Vec2::new(col as f32 * CELL, row as f32 * CELL);
            let points = [
                origin + Vec2::new(2.5, 11.5),
                origin + Vec2::new(8.0, 4.0 + wobble),
                origin + Vec2::new(13.5, 11.5),
            ];
            ui.add_shape(
                Shape::polyline(&points, PolylineColors::Single(color), 3.0).join(LineJoin::Round),
            );
        }
    }
}

fn render(
    gpu: &Gpu,
    host: &mut OffscreenHost,
    target: &wgpu::Texture,
    workload: Workload,
    phase: &mut bool,
) {
    *phase = !*phase;
    let mut app = RecordApp::new(|ui| record(ui, workload, *phase));
    host.frame_offscreen(target, 1.0, &mut app);
    poll(&gpu.device);
}

fn median(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    Some(values[values.len() / 2])
}

fn report_evidence(gpu: &Gpu, workload: Workload) {
    let target = target(&gpu.device);
    let mut host = host(gpu);
    let mut phase = false;
    let mut curve_ms = Vec::with_capacity(EVIDENCE_FRAMES);
    for frame in 0..EVIDENCE_FRAMES + 4 {
        render(gpu, &mut host, &target, workload, &mut phase);
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        if frame >= 4
            && let Some(ms) = host.gpu_pass_stats().last_kind_ms(BatchKind::Curve)
        {
            curve_ms.push(ms);
        }
    }
    let stats = host.gpu_pass_stats().last_pipeline_stats();
    let vs_per_instance = stats
        .map(|pipeline| pipeline.vertex_shader_invocations / workload.instances())
        .map(|count| count.to_string())
        .unwrap_or_else(|| "n/a".to_owned());
    eprintln!(
        "[curve_pipeline] {} instances={} vs_per_instance={vs_per_instance} \
         curve_median_ms={} pipeline={stats:?}",
        workload.label(),
        workload.instances(),
        median(&mut curve_ms)
            .map(|ms| format!("{ms:.4}"))
            .unwrap_or_else(|| "n/a".to_owned()),
    );
}

pub fn bench(c: &mut Criterion) {
    let gpu = gpu();
    eprintln!(
        "[curve_pipeline] adapter={} backend={:?} timestamp={} inside_pass={} pipeline_stats={}",
        gpu.info.name,
        gpu.info.backend,
        gpu.timing_features
            .contains(wgpu::Features::TIMESTAMP_QUERY),
        gpu.timing_features
            .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
        gpu.timing_features
            .contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
    );

    let mut group = c.benchmark_group("curve_pipeline/frame_wall");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);
    for workload in [Workload::CubicStrips, Workload::JoinChrome] {
        report_evidence(gpu, workload);
        let target = target(&gpu.device);
        let mut host = host(gpu);
        let mut phase = false;
        for _ in 0..4 {
            render(gpu, &mut host, &target, workload, &mut phase);
        }
        group.throughput(Throughput::Elements(workload.instances()));
        group.bench_function(workload.label(), |bencher| {
            bencher.iter(|| {
                render(gpu, &mut host, &target, workload, &mut phase);
                black_box(host.gpu_pass_stats());
            });
        });
    }
    group.finish();
}
