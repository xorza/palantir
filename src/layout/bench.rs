//! Layout alone over the scene: `atomic/layout/cpu/<arm>`, one run of measure and
//! arrange over the recorded tree, the measure cache warm (`cached`) or emptied first
//! (`forced_miss`). What [`integration/frame`](crate::ui::bench) pays for layout, apart from
//! record, cascade and paint.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE, Scene, SceneFrame};
use crate::host::harness::Harness;
use crate::layout::Layout;
use crate::scene::plane::Plane;
use criterion::Criterion;
use std::hint::black_box;

pub(crate) fn atomic<S: Scene>(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "layout", Scale::Sample);
    for (label, miss) in [("cached", false), ("forced_miss", true)] {
        group.bench_function(Backend::Cpu.case(label), |b| {
            let mut state = SceneFrame::<S>::default();
            let mut h = Harness::builder(BENCH_SURFACE)
                .system_scale(BENCH_DPR)
                .build();
            h.frame(|ui| state.record(ui));
            let surface = h.ui().display().logical_rect();
            let mut out = Layout::default();
            let run = |h: &mut Harness, out: &mut Layout| {
                if miss {
                    h.frames_mut().layout.cache.forget_all();
                } else {
                    h.frames_mut().layout.forget_last_run();
                }
                let frames = h.frames_mut();
                frames.layout.run(frames.ui.forest(), surface, out);
            };
            run(&mut h, &mut out);
            let rects = &out[Plane::MAIN].rect;
            assert!(
                rects.len() > 100,
                "the scene lays out {} rects",
                rects.len()
            );
            assert_eq!(
                *rects,
                h.ui().layout_tables()[Plane::MAIN].rect,
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
