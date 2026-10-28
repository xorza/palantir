//! Layout alone over the frame fixture: `atomic/layout/cpu/<arm>`, one run of measure and
//! arrange over the recorded tree, the measure cache warm (`cached`) or emptied first
//! (`forced_miss`). What [`integration/frame`](crate::ui::bench) pays for layout, apart from
//! record, cascade and paint.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::internals::frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};
use crate::internals::ui_harness::UiHarness;
use crate::layout::Layout;
use crate::scene::plane::Plane;
use criterion::Criterion;
use std::hint::black_box;

pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "layout", Scale::Sample);
    for (label, miss) in [("cached", false), ("forced_miss", true)] {
        group.bench_function(Backend::Cpu.case(label), |b| {
            let mut state = FrameFixture::default();
            let mut h = UiHarness::with_text(BENCH_SURFACE).scale(BENCH_DPR);
            h.frame(|ui| state.render(BENCH_SCALE, ui));
            let surface = h.ui.display().logical_rect();
            let mut out = Layout::default();
            let run = |h: &mut UiHarness, out: &mut Layout| {
                if miss {
                    h.engines.layout.cache.forget_all();
                } else {
                    h.engines.layout.forget_last_run();
                }
                h.engines.layout.run(h.ui.forest(), surface, out);
            };
            run(&mut h, &mut out);
            let rects = &out[Plane::MAIN].rect;
            assert!(
                rects.len() > 1000,
                "the fixture lays out {} rects",
                rects.len()
            );
            assert_eq!(
                *rects,
                h.ui.layout_tables()[Plane::MAIN].rect,
                "the frame's own layout"
            );
            b.iter(|| {
                run(&mut h, &mut out);
                black_box(&out);
            });
        });
    }
    group.finish();
}
