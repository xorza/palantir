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
//! | [`full_tree_cpu_frame_alloc_free`] | record → measure → arrange → cascade → damage over the frame bench's own tree, through real cosmic shaping. `Ui::frame` stops before the frontend, so no paint | strict zero |
//! | [`on_gpu::offscreen_frame_stays_at_driver_floor`] | a whole frame through `OffscreenHost::frame` — encode, compose, and the wgpu submission, over a still tree | the driver floor |
//! | [`on_gpu::scale_ramp_rasterizes_at_a_flat_cost_per_frame`] | the same frame under a continuous zoom: full damage, glyph and icon rasterization, both atlases' insert paths | the measured miss cost |
//!
//! The two in [`on_gpu`] read a ceiling the driver sets rather than a
//! strict zero, which is what earns them a module of their own.
//!
//! All three audit each measured frame on its own rather than summing a
//! window, so an intermittent grow-on-Nth-frame allocation (`Vec`
//! doubling, a `HashMap` rehash) fails on the frame that did it and
//! arrives with that frame's backtraces attached.

pub(crate) mod on_gpu;

use palantir::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};

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
/// Coverage stops where `Ui::frame` does, at damage. The encode and
/// compose passes need a device: the two gates below run them over the
/// whole tree, and `fixtures/renderer.rs` runs them one shape kind at a
/// time.
#[test]
fn full_tree_cpu_frame_alloc_free() {
    let mut state = FrameFixture::default();
    Audit::new()
        .text()
        .surface(BENCH_SURFACE)
        .dpr(BENCH_DPR)
        .warmup(WARMUP_FRAMES)
        .frames(MEASURE_FRAMES)
        .run(|ui| state.render(BENCH_SCALE, ui));
}
