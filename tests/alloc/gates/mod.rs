//! Coarse gates over the whole pipeline, the counterpart to the
//! fine-grained fixtures next door.
//!
//! Those audit ~20 small scenes, most of them GPU-less, so a failure can
//! name the line that allocated. These three answer what a small scene
//! cannot:
//! whether the pipeline allocates at all at *full* scale, whether the
//! wgpu floor beneath it has drifted, and what a frame costs when every
//! glyph and icon on it misses its atlas.
//!
//! | gate | covers | budget |
//! |---|---|---|
//! | [`full_tree_cpu_frame_alloc_free`] | record → measure → arrange → cascade → damage → encode → compose over the frame bench's own tree, through real cosmic shaping, on a deviceless frontend | strict zero |
//! | [`on_gpu::still_tree_frame_costs_the_empty_floor`] | a whole frame through `OffscreenHost::frame`, wgpu submission included, over a still tree | an empty scene's cost on the same adapter |
//! | [`on_gpu::scale_ramp_rasterizes_at_a_flat_cost_per_frame`] | a frame under a continuous zoom: full damage, glyph and icon rasterization, both atlases' insert paths | the measured miss cost |
//!
//! The two in [`on_gpu`] take a device, and what they count is partly
//! the driver's rather than a strict zero, which is what earns them a
//! module of their own.
//!
//! All three audit each measured frame on its own rather than summing a
//! window, so an intermittent grow-on-Nth-frame allocation (`Vec`
//! doubling, a `HashMap` rehash) fails on the frame that did it and
//! arrives with that frame's backtraces attached.

pub(crate) mod on_gpu;

use palantir::internals::{FrontendHarness, UiHarness};
use palantir::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture, FramePaint};

use crate::harness::Audit;

/// Measured, the fixture stabilizes by frame 4 — at 1 it still leaks
/// ~10 blocks — so this is margin. Too short is safe in the direction
/// that matters: the leftovers land inside the measured window and trip
/// the gate rather than hiding under it.
pub(crate) const WARMUP_FRAMES: usize = 16;
/// Long enough for a once-every-N-frames allocation to land inside the
/// window rather than after it.
pub(crate) const MEASURE_FRAMES: usize = 256;

/// Pins the `AGENTS.md` claim: "Per-frame allocation is a real metric.
/// Steady-state must be heap-alloc-free after warmup." Strict zero,
/// because everything on this path is ours.
///
/// Renders the frame bench's tree at its surface and dpr, through real
/// cosmic shaping rather than the mono fallback — so what clears here is
/// the tree that bench times, not a smaller stand-in whose quieter
/// caches prove less.
///
/// Every frame runs the whole CPU pipeline, through encode and compose
/// on the deviceless frontend. The tree stands still, so its frames plan
/// no paint after the first; each then repaints the whole scene anyway,
/// as the frame bench's `cached_cpu` arm does, so the encoder and the
/// composer see every node on every measured frame.
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
