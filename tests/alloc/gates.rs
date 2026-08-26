//! Coarse gates over the whole pipeline, the counterpart to the
//! fine-grained fixtures next door.
//!
//! Those audit ~20 small scenes, GPU-less, so a failure can name the
//! line that allocated. These two answer what a small scene cannot:
//! whether the pipeline allocates at all at *full* scale, and whether
//! the wgpu floor beneath it has drifted.
//!
//! | gate | covers | budget |
//! |---|---|---|
//! | [`full_tree_cpu_frame_alloc_free`] | record → measure → arrange → cascade → damage over the frame bench's own tree, through real cosmic shaping. `Ui::frame` stops before the frontend, so no paint | strict zero |
//! | [`offscreen_frame_stays_at_driver_floor`] | a whole frame through `OffscreenHost::frame_offscreen` — encode, compose, and the wgpu submission | the driver floor |
//!
//! Both audit each measured frame on its own rather than summing a
//! window, so an intermittent grow-on-Nth-frame allocation (`Vec`
//! doubling, a `HashMap` rehash) fails on the frame that did it and
//! arrives with that frame's backtraces attached.

use glam::UVec2;
use palantir::internals::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, RecordApp, headless_test_gpu};
use palantir::{Color, FrameFixture, OffscreenHost};

use crate::harness::Audit;

/// Measured, the fixture stabilizes by frame 4 — at 1 it still leaks
/// ~10 blocks — so this is margin. Too short is safe in the direction
/// that matters: the leftovers land inside the measured window and trip
/// the gate rather than hiding under it.
const WARMUP_FRAMES: usize = 16;
/// Long enough for a once-every-N-frames allocation to land inside the
/// window rather than after it.
const MEASURE_FRAMES: usize = 256;

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
/// compose passes need a `Frontend`; the gate below runs them as part of
/// a whole frame, and `fixtures/renderer.rs` audits them directly.
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

/// Driver floor on the current wgpu/cosmic-text pin. Bump it if a driver
/// upgrade or a deliberate palantir change moves the baseline; trip the
/// gate otherwise. All current attribution is wgpu_core/wgpu_hal — no
/// palantir-side per-frame allocs on this path.
const RENDER_BLOCKS_PER_FRAME_MAX: u64 = 35;

/// This gate's own surface and tree, deliberately smaller than the one
/// above: what it pins is the driver's per-frame floor, which scales
/// with submissions rather than with node count. A bigger tree would
/// only make the same number slower to reach.
const RENDER_SURFACE: UVec2 = UVec2::new(1280, 800);
const RENDER_NODE_SCALE: usize = 6;

/// **Not** strict zero, and cannot be: every wgpu submission allocates a
/// `CommandEncoder` Arc, a `CommandBuffer` Arc, the queue's in-flight
/// `Vec` push, and per-pass scratch from `wgpu_hal`. So the gate catches
/// *drift* from that floor — a palantir regression, or a wgpu bump worth
/// looking at.
///
/// The leased test device carries no timestamp or pipeline-statistics
/// features, which matters: the queries an instrumented device runs
/// allocate per frame, and that is the very thing being counted here.
#[test]
fn offscreen_frame_stays_at_driver_floor() {
    let gpu = headless_test_gpu();
    // The public offscreen path always copies from its backbuffer, so
    // the floor pinned here excludes the direct-present path.
    let mut host = OffscreenHost::builder(gpu.device.clone(), gpu.queue.clone()).build();
    host.ui().theme_mut().window_clear = Color::TRANSPARENT;

    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.alloc_gate.render.target"),
        size: wgpu::Extent3d {
            width: RENDER_SURFACE.x,
            height: RENDER_SURFACE.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let mut state = FrameFixture::default();
    let report = Audit::new()
        .warmup(WARMUP_FRAMES)
        .frames(MEASURE_FRAMES)
        .budget(RENDER_BLOCKS_PER_FRAME_MAX)
        .run_frames(|| {
            host.frame_offscreen(
                &target,
                BENCH_DPR,
                &mut RecordApp::new(|ui| state.render(RENDER_NODE_SCALE, ui)),
            );
            // Draining here is what puts GPU execution inside the frame
            // that submitted it instead of the next one's window.
            gpu.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .expect("device poll");
        });

    // A gate that reads zero has stopped measuring, and only the number
    // says so.
    assert!(
        report.worst > 0,
        "counted no allocation at all across {MEASURE_FRAMES} frames — the wgpu \
         submission path allocates, so this gate is no longer watching it",
    );
}
