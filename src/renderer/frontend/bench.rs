//! Record-to-compose comparison for repeated solid and gradient chrome.

use crate::bench::Run;
use crate::internals::harness::UiHarness;
use crate::internals::harness::frontend_harness::FrontendHarness;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::color::RgbaF32;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
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
            Self::Solid => Brush::Solid(RgbaF32::srgb(0.12, 0.24, 0.48)),
            Self::Gradient => Brush::Linear(LinearGradient::two_stop(
                0.5,
                RgbaF32::hex(0x1a1a2e),
                RgbaF32::hex(0x4c5cdb),
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
    frontend: FrontendHarness,
    start: Instant,
}

impl GradientBench {
    fn new() -> Self {
        Self {
            frontend: FrontendHarness::new(UiHarness::new(PHYSICAL)),
            start: Instant::now(),
        }
    }

    fn frame(&mut self, fill_case: FillCase) -> usize {
        let background = fill_case.background();
        self.frontend.harness.at(self.start.elapsed());
        let report = self.frontend.frame(|ui| {
            for row in 0..ROWS {
                Block::new()
                    .id_salt(row)
                    .size((8.0, 8.0))
                    .background(background.clone())
                    .show(ui);
            }
        });
        if report.plan.is_none() {
            self.frontend.paint_full();
        }
        self.frontend.frontend.buffer.quads.len()
    }
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.subgroup(c, "repeated_chrome");
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
                .frontend
                .harness
                .ui
                .forest()
                .record_store
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
