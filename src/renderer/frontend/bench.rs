//! Record to compose over [`ROWS`] blocks of repeated chrome: what a gradient fill costs the frontend over a solid one.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::internals::ui_harness::UiHarness;
use crate::internals::ui_harness::frontend_harness::FrontendHarness;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use criterion::{Criterion, Throughput};
use glam::UVec2;
use std::hint::black_box;
use std::time::Instant;

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

    fn chrome(self) -> Chrome {
        let fill = match self {
            Self::Solid => Brush::Solid(RgbaF32::srgb(0.12, 0.24, 0.48)),
            Self::Gradient => Brush::Linear(LinearGradient::two_stop(
                0.5,
                RgbaF32::hex(0x1a1a2e),
                RgbaF32::hex(0x4c5cdb),
            )),
        };
        Chrome::fill(fill)
    }
}

#[derive(Debug)]
struct ChromeBench {
    frontend: FrontendHarness,
    start: Instant,
}

impl ChromeBench {
    fn new() -> Self {
        Self {
            frontend: FrontendHarness::new(UiHarness::new(PHYSICAL)),
            start: Instant::now(),
        }
    }

    fn frame(&mut self, fill_case: FillCase) -> usize {
        let chrome = fill_case.chrome();
        self.frontend.harness.at(self.start.elapsed());
        let report = self.frontend.frame(|ui| {
            for row in 0..ROWS {
                Block::new()
                    .id_salt(row)
                    .size((8.0, 8.0))
                    .chrome(chrome.clone())
                    .show(ui);
            }
        });
        if report.plan.is_none() {
            self.frontend.paint_full();
        }
        self.frontend.frontend.buffer.quads.len()
    }
}

/// `integration/frontend/cpu/repeated_chrome/<fill>`: record to compose over [`ROWS`] blocks,
/// solid and gradient filled. Per block.
pub(crate) fn integration(c: &mut Criterion) {
    let mut group = Kind::Integration.group(c, "frontend", Scale::Sample);
    group.throughput(Throughput::Elements(ROWS as u64));
    for fill_case in FillCase::ALL {
        let case = format!("repeated_chrome/{}", fill_case.label());
        group.bench_function(Backend::Cpu.case(&case), |b| {
            let mut fixture = ChromeBench::new();
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
            b.iter(|| black_box(fixture.frame(fill_case)));
        });
    }
    group.finish();
}
