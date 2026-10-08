//! GPU curve-pipeline benchmark: `cubic_strips` (one short cubic per cell, one instance) and `join_chrome` (a three-point polyline per cell, two segments and one join).
//!
//! Each iteration toggles a control point so damage streams every curve to the backend, then waits for the GPU. The keep-or-revert signal is the minimum curve-batch timestamp, printed with the median before each case.

#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::bench::Run;
use crate::bench::summary::Summary;
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::bench_gpu::{BenchGpu, BenchTarget, Timing};
use crate::host::offscreen::OffscreenHost;
use crate::internals::record_app::RecordApp;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::style::LineJoin;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::{Configure, Sizing, Vec2};
use criterion::{Criterion, Throughput};
use std::hint::black_box;

const PHYSICAL: glam::UVec2 = glam::UVec2::new(1024, 1024);
const GRID: u32 = 64;
const CELL: f32 = 16.0;
const CUBIC_INSTANCES: u64 = (GRID * GRID) as u64;
const JOIN_INSTANCES: u64 = (GRID * GRID * 3) as u64;
/// Frames before sampling; an integrated GPU needs a long ramp for clocks to settle.
const WARMUP_FRAMES: usize = 128;
const EVIDENCE_FRAMES: usize = 256;

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

fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::Instrumented)
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
    let color = RgbaF32::srgb(0.2, 0.8, 1.0);
    let wobble = if phase { 0.125 } else { -0.125 };
    for row in 0..GRID {
        for col in 0..GRID {
            let origin = Vec2::new(col as f32 * CELL, row as f32 * CELL);
            ui.add_shape(Shape::cubic_bezier(
                origin + Vec2::new(2.0, 8.0),
                origin + Vec2::new(5.0, 5.5 + wobble),
                origin + Vec2::new(11.0, 10.5),
                origin + Vec2::new(14.0, 8.0),
                Stroke::new(color, 2.0),
            ));
        }
    }
}

fn record_joins(ui: &mut Ui, phase: bool) {
    let color = RgbaF32::srgba(0.3, 1.0, 0.5, 0.75);
    let wobble = if phase { 0.125 } else { -0.125 };
    for row in 0..GRID {
        for col in 0..GRID {
            let origin = Vec2::new(col as f32 * CELL, row as f32 * CELL);
            let points = [
                origin + Vec2::new(2.5, 11.5),
                origin + Vec2::new(8.0, 4.0 + wobble),
                origin + Vec2::new(13.5, 11.5),
            ];
            ui.add_shape(Shape::polyline(&points, Stroke::new(color, 3.0)).join(LineJoin::Round));
        }
    }
}

fn render(
    gpu: &BenchGpu,
    host: &mut OffscreenHost,
    target: &BenchTarget,
    workload: Workload,
    phase: &mut bool,
) {
    *phase = !*phase;
    let mut app = RecordApp::new(|ui| record(ui, workload, *phase));
    host.frame(target.as_target(), 1.0, &mut app);
    gpu.wait();
}

fn report_evidence(gpu: &BenchGpu, workload: Workload) {
    let target = gpu.target("palantir.curve_pipeline_bench.target", PHYSICAL);
    let mut host = gpu.plain_host(true);
    let mut phase = false;
    let mut samples = Vec::with_capacity(EVIDENCE_FRAMES);
    for frame in 0..WARMUP_FRAMES + EVIDENCE_FRAMES {
        render(gpu, &mut host, &target, workload, &mut phase);
        gpu.poll();
        if frame >= WARMUP_FRAMES
            && let Some(time) = host.gpu_pass_stats().last_kind(BatchKind::Curve)
        {
            samples.push(time);
        }
    }
    let stats = host.gpu_pass_stats().last_pipeline_stats();
    let vs_per_instance = stats
        .map(|pipeline| pipeline.vertex_shader_invocations / workload.instances())
        .map_or_else(|| "n/a".to_owned(), |count| count.to_string());
    eprintln!(
        "[curve_pipeline] {} instances={} vs_per_instance={vs_per_instance} curve {} \
         pipeline={stats:?}",
        workload.label(),
        workload.instances(),
        Summary::of(&mut samples).map_or_else(|| "n/a".to_owned(), |s| s.to_string()),
    );
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let gpu = gpu();
    eprintln!("[curve_pipeline] {}", gpu.summary());

    let mut group = run.subgroup(c, "frame_wall");
    for workload in [Workload::CubicStrips, Workload::JoinChrome] {
        report_evidence(gpu, workload);
        let target = gpu.target("palantir.curve_pipeline_bench.target", PHYSICAL);
        let mut host = gpu.plain_host(true);
        let mut phase = false;
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
