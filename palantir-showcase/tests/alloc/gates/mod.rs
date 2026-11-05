//! Coarse gates over the whole pipeline, counterpart to the small fixtures next door: full-scale allocation, the wgpu floor, and a frame where every glyph and icon misses its atlas.
//!
//! | gate | covers | budget |
//! |---|---|---|
//! | [`full_tree_cpu_frame_alloc_free`] | record through compose over each of the showcase's pages, real cosmic shaping, deviceless | strict zero |
//! | [`backdrop::backdrop_cpu_frames_alloc_free`] | the same with three panes over it, plain, frosted and glass, still and with a pane going stale every frame | strict zero |
//! | [`on_gpu::still_tree_frame_costs_the_empty_floor`] | a whole frame through `OffscreenHost::frame` over a still tree | an empty scene's cost through the same target |
//! | [`on_gpu::scale_ramp_rasterizes_at_a_flat_cost_per_frame`] | a frame under continuous zoom: full damage, glyph and icon rasterization | the measured miss cost |
//!
//! The two in [`on_gpu`] take a device, so what they count is partly the driver's, not a strict zero.
//!
//! Each measured frame is audited on its own, so a grow-on-Nth-frame allocation fails on its frame with backtraces.

mod backdrop;
mod on_gpu;

use palantir::FramePaint;
use palantir::bench::scene::{BENCH_DPR, BENCH_SURFACE};
use palantir::{App, Depth, Harness};
use palantir_showcase::{MAIN_WINDOW, Shell};

use crate::harness::Audit;

/// Frames before a gate measures, as a scene allocates while it settles; too short is the safe direction.
const WARMUP_FRAMES: usize = 16;
/// Long enough for a once-every-N-frames allocation to land inside the window.
const MEASURE_FRAMES: usize = 256;

/// Pins the `AGENTS.md` claim that steady state is heap-alloc-free after warmup; strict zero, since everything on this path is ours.
///
/// Renders each of the showcase's pages through real cosmic shaping; every frame repaints the whole scene through compose on the deviceless frontend. The window holds a page's first expiries, which bring the buffers its first frames shaped into the kept store and the ticket pool.
#[test]
fn full_tree_cpu_frame_alloc_free() {
    for page in 0..Shell::PAGE_COUNT {
        println!("page {page}");
        let mut shell = Shell::open(page);
        let mut frontend = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .depth(Depth::Compose)
            .build();
        let mut recorded = 0;
        Audit::new()
            .warmup(WARMUP_FRAMES)
            .frames(MEASURE_FRAMES)
            .run_frames(|| {
                let report = frontend.frame(|ui| {
                    recorded += 1;
                    shell.record(MAIN_WINDOW, ui);
                });
                if report.paint() == FramePaint::Skip {
                    frontend.paint_full();
                }
            });
        // A page with a paint-only animation, as Materials and Motion, records in its first frames
        // alone, and the window audits its paint.
        assert!(recorded > 0, "page {page} recorded no frame");
    }
}
