//! GPU curve-pipeline benchmark: `cubic_strips` (one short cubic per cell, one instance) and `join_chrome` (a three-point polyline per cell, two segments and one join).
//!
//! Each iteration toggles a control point so damage streams every curve to the backend, then waits for the GPU. Criterion records the curve batch's GPU timestamp, not the frame's wall time.

use crate::bench::backends::Backends;
use crate::bench::fixture;
use crate::bench::kind::{Kind, Scale};
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::host::headless_window::HeadlessWindow;
use crate::internals::record_app::RecordApp;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::style::LineJoin;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use criterion::{Criterion, Throughput};
use glam::Vec2;

const PHYSICAL: glam::UVec2 = glam::UVec2::new(1024, 1024);
const GRID: u32 = 64;
const CELL: f32 = 16.0;
const CUBIC_INSTANCES: u64 = (GRID * GRID) as u64;
const JOIN_INSTANCES: u64 = (GRID * GRID * 3) as u64;

#[derive(Clone, Copy, Debug)]
enum Workload {
    CubicStrips,
    Join,
}

impl Workload {
    const fn label(self) -> &'static str {
        match self {
            Self::CubicStrips => "cubic_strips",
            Self::Join => "join",
        }
    }

    const fn instances(self) -> u64 {
        match self {
            Self::CubicStrips => CUBIC_INSTANCES,
            Self::Join => JOIN_INSTANCES,
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
            Workload::Join => record_joins(ui, phase),
        });
}

fn record_cubics(ui: &mut Ui, phase: bool) {
    let color = RgbaF32::srgb(0.2, 0.8, 1.0);
    let wobble = if phase { 0.125 } else { -0.125 };
    for row in 0..GRID {
        for col in 0..GRID {
            let origin = Vec2::new(col as f32 * CELL, row as f32 * CELL);
            ui.add_shape(Shape::cubic_bezier(
                CubicBezier {
                    p0: origin + Vec2::new(2.0, 8.0),
                    p1: origin + Vec2::new(5.0, 5.5 + wobble),
                    p2: origin + Vec2::new(11.0, 10.5),
                    p3: origin + Vec2::new(14.0, 8.0),
                },
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

/// A window and the toggle that keeps each frame repainting.
#[derive(Debug)]
struct Fixture {
    window: HeadlessWindow,
    phase: bool,
}

impl Fixture {
    fn new(gpu: &BenchGpu) -> Self {
        Self {
            window: fixture::plain_window(gpu, PHYSICAL, true),
            phase: false,
        }
    }

    fn render(&mut self, gpu: &BenchGpu, workload: Workload) {
        self.phase = !self.phase;
        let phase = self.phase;
        let mut app = RecordApp::new(|ui| record(ui, workload, phase));
        self.window.frame(&mut app);
        gpu.wait();
    }
}

/// `atomic/curve_pipeline/gpu/<workload>`: the curve batch of one frame on the device, from its
/// timestamps. Per curve instance.
pub(crate) fn atomic(c: &mut Criterion, backends: Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "curve_pipeline", Scale::Sample);
    for workload in [Workload::CubicStrips, Workload::Join] {
        group.throughput(Throughput::Elements(workload.instances()));
        group.bench_function(backend.case(workload.label()), |bencher| {
            let gpu = gpu();
            let mut fixture = Fixture::new(gpu);
            // The timestamp readback lags a frame, so a few frames run before the first reads.
            let timed = (0..8).any(|_| {
                fixture.render(gpu, workload);
                gpu.poll();
                fixture
                    .window
                    .host()
                    .gpu_pass_stats()
                    .last_kind(BatchKind::Curve)
                    .is_some()
            });
            assert!(
                timed,
                "no curve batch timestamp: the adapter needs in-pass timestamps"
            );
            bencher.iter_custom(|iters| {
                (0..iters)
                    .map(|_| {
                        fixture.render(gpu, workload);
                        gpu.poll();
                        fixture
                            .window
                            .host()
                            .gpu_pass_stats()
                            .last_kind(BatchKind::Curve)
                            .expect("a curve batch timestamp every frame once one arrived")
                    })
                    .sum()
            });
        });
    }
    group.finish();
}
