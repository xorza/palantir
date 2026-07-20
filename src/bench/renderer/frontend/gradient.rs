//! Record-to-compose comparison for repeated solid and gradient chrome.

use crate::display::Display;
use crate::forest::element::Configure;
use crate::primitives::background::Background;
use crate::primitives::brush::{Brush, LinearGradient};
use crate::primitives::color::{Color, ColorU8};
use crate::renderer::frontend::Frontend;
use crate::ui::Ui;
use crate::ui::frame::FrameStamp;
use crate::ui::frame_report::{RenderKind, RenderPlan};
use crate::widgets::frame::Frame;
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::UVec2;
use std::hint::black_box;
use std::time::{Duration, Instant};

const ROWS: usize = 1_024;
const PHYSICAL: UVec2 = UVec2::new(128, 128);

#[derive(Clone, Copy, Debug)]
enum FillCase {
    Solid,
    Gradient,
}

impl FillCase {
    const ALL: [Self; 2] = [Self::Solid, Self::Gradient];

    const fn label(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Gradient => "gradient",
        }
    }

    fn background(self) -> Background {
        let fill = match self {
            Self::Solid => Brush::Solid(Color::rgb(0.12, 0.24, 0.48)),
            Self::Gradient => Brush::Linear(LinearGradient::two_stop(
                0.5,
                ColorU8::hex(0x1a1a2e),
                ColorU8::hex(0x4c5cdb),
            )),
        };
        Background {
            fill,
            ..Background::default()
        }
    }
}

#[derive(Debug)]
struct GradientBench {
    ui: Ui,
    frontend: Frontend,
    start: Instant,
}

impl GradientBench {
    fn new() -> Self {
        Self {
            ui: Ui::for_test(),
            frontend: Frontend::for_test(),
            start: Instant::now(),
        }
    }

    fn frame(&mut self, fill_case: FillCase) -> usize {
        let background = fill_case.background();
        let display = Display::from_physical(PHYSICAL, 1.0);
        let report = self
            .ui
            .record_acked(FrameStamp::new(display, self.start.elapsed()), |ui| {
                for row in 0..ROWS {
                    Frame::new()
                        .id_salt(row)
                        .size((8.0, 8.0))
                        .background(background.clone())
                        .show(ui);
                }
            });
        let plan = report.plan.unwrap_or(RenderPlan {
            clear: Color::BLACK,
            kind: RenderKind::Full,
        });
        self.frontend.build(self.ui.frame_scene(), plan);
        self.frontend.buffer.quads.len()
    }
}

pub fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("gradient/repeated_chrome");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.throughput(Throughput::Elements(ROWS as u64));

    for fill_case in FillCase::ALL {
        let mut fixture = GradientBench::new();
        for _ in 0..4 {
            black_box(fixture.frame(fill_case));
        }
        let expected_gradients = match fill_case {
            FillCase::Solid => 0,
            FillCase::Gradient => 1,
        };
        assert_eq!(
            fixture
                .ui
                .record_store
                .payloads
                .borrow()
                .gradients
                .records
                .len(),
            expected_gradients,
        );
        group.bench_with_input(
            BenchmarkId::from_parameter(fill_case.label()),
            &fill_case,
            |b, &fill_case| b.iter(|| black_box(fixture.frame(fill_case))),
        );
    }
    group.finish();
}
