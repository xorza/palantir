//! Layout alone over the scene: `atomic/layout/cpu/<arm>`, one run of measure and
//! arrange over the recorded tree, the measure cache warm (`cached`) or emptied first
//! (`forced_miss`). What [`integration/frame`](crate::ui::bench) pays for layout, apart from
//! record, cascade and paint. `grid_forced_miss` lays out a grid the scene has none of.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE, Scene, SceneFrame};
use crate::host::harness::Harness;
use crate::layout::Layout;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::track::Track;
use crate::scene::plane::Plane;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::grid::Grid;
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
                    h.frames_mut().layout_mut().cache.forget_all();
                } else {
                    h.frames_mut().layout_mut().forget_last_run();
                }
                h.frames_mut().run_layout(surface, out);
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
                h.ui().layout()[Plane::MAIN].rect,
                "the frame's own layout"
            );
            b.iter(|| {
                run(&mut h, &mut out);
                black_box(&out);
            });
        });
    }
    group.bench_function(Backend::Cpu.case("grid_forced_miss"), |b| {
        let mut h = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .build();
        h.frame(grid);
        let surface = h.ui().display().logical_rect();
        let mut out = Layout::default();
        let run = |h: &mut Harness, out: &mut Layout| {
            h.frames_mut().layout_mut().cache.forget_all();
            h.frames_mut().run_layout(surface, out);
        };
        run(&mut h, &mut out);
        assert_eq!(
            out[Plane::MAIN].rect.len(),
            GRID_SIDE * GRID_SIDE + 2,
            "the frame's root, the grid and its cells"
        );
        b.iter(|| {
            run(&mut h, &mut out);
            black_box(&out);
        });
    });
    group.finish();
}

/// Cells along each side of [`grid`].
const GRID_SIDE: usize = 32;

/// A grid of Hug tracks, [`GRID_SIDE`] a side, a fixed block in each cell and none spanning.
fn grid(ui: &mut Ui) {
    Grid::new()
        .cols([Track::HUG; GRID_SIDE])
        .rows([Track::HUG; GRID_SIDE])
        .show(ui, |ui| {
            for row in 0..GRID_SIDE as u16 {
                for col in 0..GRID_SIDE as u16 {
                    Block::new()
                        .id_salt((row, col))
                        .size((8.0, 8.0))
                        .grid_cell(GridCell::at(row, col))
                        .show(ui);
                }
            }
        });
}
