//! Strict per-frame allocation invariant for aperture's record/measure/
//! arrange/cascade/encode pipeline (no GPU). Pinning test for the
//! `AGENTS.md` claim: "Per-frame allocation is a real metric.
//! Steady-state must be heap-alloc-free after warmup."
//!
//! Runs the shared `frame_fixture` workload through `Ui::record`, warms
//! up so retained scratch / caches stabilize, then measures heap-block
//! delta over a batch of steady-state frames. **Fails on any non-zero
//! delta** — aperture-side regressions show up here.
//!
//! For the GPU submission path (wgpu backend allocations under
//! `WgpuBackend::submit`), see `alloc_free_gpu.rs` — driver overhead
//! has a different floor and different semantics.
//!
//! Uses `dhat` as the global allocator (10-30x overhead — never use
//! this binary for timing).
//!
//! Run with: `cargo bench --bench alloc_free --features internals`
//! Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_free --features internals`

use aperture::{Display, Ui, bench::FrameFixture};
use glam::UVec2;
use std::hint::black_box;
use std::time::Duration;

use crate::ui::frame::FrameStamp;

// Uses `Ui::default()` (mono-fallback shaper, self-contained) and warms
// manually below via `WARMUP_FRAMES` before measuring.

const WARMUP_FRAMES: usize = 16;
// 256 measure frames so an intermittent grow-on-Nth-frame allocation
// (Vec doubling, HashMap rehash) isn't lost between two snapshots.
const MEASURE_FRAMES: usize = 256;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const SCALE: f32 = 2.0;
// Smaller than `frame.rs`'s BENCH_SCALE=32 because the alloc-free
// viewport is 1280x800 instead of 3840x4800 — matches `examples/frame_visual.rs`.
const NODE_SCALE: usize = 6;

pub fn bench() {
    let want_dump = std::env::var("DHAT_DUMP").ok().as_deref() == Some("1");
    let _profiler = if want_dump {
        Some(dhat::Profiler::new_heap())
    } else {
        Some(dhat::Profiler::builder().testing().build())
    };

    let display = Display::from_physical(PHYSICAL, SCALE);
    let mut ui = Ui::default();
    let mut state = FrameFixture::default();

    for _ in 0..WARMUP_FRAMES {
        black_box(ui.record(FrameStamp::new(display, Duration::ZERO), |ui| {
            state.render(NODE_SCALE, ui)
        }));
    }
    let before = dhat::HeapStats::get();
    for _ in 0..MEASURE_FRAMES {
        black_box(ui.record(FrameStamp::new(display, Duration::ZERO), |ui| {
            state.render(NODE_SCALE, ui)
        }));
    }
    let after = dhat::HeapStats::get();

    let block_delta = after.total_blocks - before.total_blocks;
    let byte_delta = after.total_bytes - before.total_bytes;

    println!(
        "alloc_free: warmup={WARMUP_FRAMES} measure={MEASURE_FRAMES} \
         ({PHYSICAL:?} @ {SCALE}x, node_scale={NODE_SCALE})"
    );
    println!(
        "  record-only           {block_delta:6} blocks  {byte_delta:10} bytes  \
         ({:5.2}/frame, limit strict zero)",
        block_delta as f64 / MEASURE_FRAMES as f64,
    );

    let ok = block_delta == 0 && byte_delta == 0;

    // Drop the profiler explicitly so DHAT_DUMP=1 writes dhat-heap.json
    // before we exit (process::exit skips Drop).
    drop(_profiler);

    if !ok {
        eprintln!();
        eprintln!(
            "FAIL: record-only must be strictly allocation-free; got {:.2} blocks/frame.",
            block_delta as f64 / MEASURE_FRAMES as f64
        );
        eprintln!();
        eprintln!("Inspect call sites with:");
        eprintln!("  DHAT_DUMP=1 cargo bench --bench alloc_free --features internals");
        eprintln!("  open dhat-heap.json at https://nnethercote.github.io/dh_view/");
        std::process::exit(1);
    }

    println!();
    println!("PASS: aperture CPU pipeline is allocation-free in steady state.");
}
