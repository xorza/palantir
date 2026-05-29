//! Steady-state allocation count for the resize path. Mirrors
//! `alloc_free.rs` but rotates the `Display` size each frame to bust
//! `MeasureCache` / `CascadeCache` / text-shaping caches the way the
//! `frame/resizing_cpu` arm does. **Not strict-zero** — this bench
//! measures, doesn't assert. Use the output to find which call sites
//! are still allocating after warmup.
//!
//! Uses `Ui::for_test_text()` (real cosmic-text), NOT `Ui::default()`
//! (mono fallback): the fallback emits a constant paint count across
//! sizes, so `CascadeCache::capture` reuses its arena slots in place
//! and the bench reports a misleading 0 blocks/frame. Real shaping
//! reflows text per size, drifting the paint count and exercising the
//! capture evict/append path the live `frame/resizing_cpu` arm hits.
//! That dependency is why this bench requires the `internals` feature.
//!
//! Run with: `cargo bench --bench alloc_resize --features internals`
//! Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_resize --features internals`

#[path = "support/frame_fixture.rs"]
mod fixture;

use fixture::{FormState, build_ui};
use glam::UVec2;
use palantir::{Display, FrameStamp, Ui};
use std::hint::black_box;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const WARMUP_FRAMES: usize = 32;
const MEASURE_FRAMES: usize = 256;
const SCALE: f32 = 2.0;
// Match `frame.rs::BENCH_SCALE` / `RESIZE_POOL` so the workload is
// the same shape `frame/resizing_cpu` measures (~800 nodes, ~500
// text shapes).
const NODE_SCALE: usize = 32;

const RESIZE_POOL: &[UVec2] = &[
    UVec2::new(3200, 4400),
    UVec2::new(3840, 4800),
    UVec2::new(3520, 4600),
    UVec2::new(4160, 5000),
];

/// Continuous-drag mode: every frame is a unique width, modelling a
/// user dragging the window edge. With ~256 unique sizes the text /
/// measure / cascade caches never hit on the same width twice, so
/// any per-frame allocation surface stays visible.
fn continuous_size(frame: usize) -> UVec2 {
    let base = UVec2::new(3520, 4600);
    let dx = ((frame * 7) % 800) as i32 - 400;
    UVec2::new((base.x as i32 + dx).max(800) as u32, base.y)
}

fn main() {
    let want_dump = std::env::var("DHAT_DUMP").ok().as_deref() == Some("1");
    let _profiler = if want_dump {
        Some(dhat::Profiler::new_heap())
    } else {
        Some(dhat::Profiler::builder().testing().build())
    };

    let mut ui = Ui::for_test_text();
    let mut state = FormState::default();

    // Two arms: pool-rotation (matches `frame/resizing_cpu` exactly)
    // and continuous-drag (every frame a unique width — models a real
    // user dragging the window edge, no cache hits possible).
    let mut run = |label: &str, size: &mut dyn FnMut(usize) -> UVec2| {
        for f in 0..WARMUP_FRAMES {
            let display = Display::from_physical(size(f), SCALE);
            black_box(
                ui.frame(FrameStamp::new(display, std::time::Duration::ZERO), |ui| {
                    build_ui(&mut state, NODE_SCALE, ui)
                }),
            );
        }
        let before = dhat::HeapStats::get();
        for f in 0..MEASURE_FRAMES {
            let display = Display::from_physical(size(f + WARMUP_FRAMES), SCALE);
            black_box(
                ui.frame(FrameStamp::new(display, std::time::Duration::ZERO), |ui| {
                    build_ui(&mut state, NODE_SCALE, ui)
                }),
            );
        }
        let after = dhat::HeapStats::get();

        let block_delta = after.total_blocks - before.total_blocks;
        let byte_delta = after.total_bytes - before.total_bytes;
        println!(
            "  {label:20} {block_delta:6} blocks  {byte_delta:10} bytes  \
             ({:7.2} blocks/frame, {:9.0} bytes/frame)",
            block_delta as f64 / MEASURE_FRAMES as f64,
            byte_delta as f64 / MEASURE_FRAMES as f64,
        );
    };

    println!(
        "alloc_resize: warmup={WARMUP_FRAMES} measure={MEASURE_FRAMES} (node_scale={NODE_SCALE})"
    );

    run("pool-rotation", &mut |f| RESIZE_POOL[f % RESIZE_POOL.len()]);
    run("continuous-drag", &mut continuous_size);

    drop(_profiler);
}
