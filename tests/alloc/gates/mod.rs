//! Coarse gates over the whole pipeline, counterpart to the small fixtures next door: full-scale allocation, the wgpu floor, and a frame where every glyph and icon misses its atlas.
//!
//! | gate | covers | budget |
//! |---|---|---|
//! | [`full_tree_cpu_frame_alloc_free`] | record through compose over the frame bench's tree, real cosmic shaping, deviceless | strict zero |
//! | [`on_gpu::still_tree_frame_costs_the_empty_floor`] | a whole frame through `OffscreenHost::frame` over a still tree | an empty scene's cost through the same target |
//! | [`on_gpu::scale_ramp_rasterizes_at_a_flat_cost_per_frame`] | a frame under continuous zoom: full damage, glyph and icon rasterization | the measured miss cost |
//!
//! The two in [`on_gpu`] take a device, so what they count is partly the driver's, not a strict zero.
//!
//! Each measured frame is audited on its own, so a grow-on-Nth-frame allocation fails on its frame with backtraces.

pub(crate) mod on_gpu;

use palantir::FramePaint;
use palantir::internals::frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};
use palantir::internals::harness::UiHarness;
use palantir::internals::harness::frontend_harness::FrontendHarness;

use crate::harness::Audit;

/// The fixture stabilizes by frame 4 (at 1 it leaks ~10 blocks); too short is the safe direction.
pub(crate) const WARMUP_FRAMES: usize = 16;
/// Long enough for a once-every-N-frames allocation to land inside the window.
pub(crate) const MEASURE_FRAMES: usize = 256;

/// Pins the `AGENTS.md` claim that steady state is heap-alloc-free after warmup; strict zero, since everything on this path is ours.
///
/// Renders the frame bench's tree through real cosmic shaping; every frame repaints the whole scene through compose on the deviceless frontend.
#[test]
fn full_tree_cpu_frame_alloc_free() {
    let mut state = FrameFixture::default();
    let mut frontend = FrontendHarness::new(UiHarness::with_text(BENCH_SURFACE).scale(BENCH_DPR));
    Audit::new()
        .warmup(WARMUP_FRAMES)
        .frames(MEASURE_FRAMES)
        .run_frames(|| {
            let mut recorded = false;
            let report = frontend.frame(|ui| {
                recorded = true;
                state.render(BENCH_SCALE, ui);
            });
            assert!(recorded, "a still tree's frame still records");
            if report.paint() == FramePaint::Skip {
                frontend.paint_full();
            }
        });
}
