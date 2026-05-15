//! Strict per-frame allocation invariant for palantir's record/measure/
//! arrange/cascade/encode pipeline (no GPU). Pinning test for the
//! `CLAUDE.md` claim: "Per-frame allocation is a real metric.
//! Steady-state must be heap-alloc-free after warmup."
//!
//! Runs a small but realistic UI through `Ui::run_frame`, warms up so
//! retained scratch / caches stabilize, then measures heap-block delta
//! over a batch of steady-state frames. **Fails on any non-zero
//! delta** — palantir-side regressions show up here.
//!
//! For the GPU submission path (wgpu backend allocations under
//! `WgpuBackend::submit`), see `alloc_free_gpu.rs` — driver overhead
//! has a different floor and different semantics.
//!
//! Uses `dhat` as the global allocator (10-30x overhead — never use
//! this binary for timing).
//!
//! Run with: `cargo bench --bench alloc_free`
//! Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_free`

use glam::UVec2;
use palantir::{
    Align, Button, Configure, Display, Frame, FrameStamp, Justify, Panel, Sizing, Text, TextShaper,
    TextStyle, Ui, new_handle,
};
use std::hint::black_box;

/// Local mono-fallback `Ui` constructor. `internals::new_ui` is gated
/// behind the `internals` feature; this bench doesn't enable it.
fn new_ui() -> Ui {
    Ui::new(TextShaper::default(), new_handle())
}

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const WARMUP_FRAMES: usize = 16;
// 256 measure frames so an intermittent grow-on-Nth-frame allocation
// (Vec doubling, HashMap rehash) isn't lost between two snapshots.
const MEASURE_FRAMES: usize = 256;

const PHYSICAL: UVec2 = UVec2::new(1280, 800);
const SCALE: f32 = 2.0;

fn build_ui(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(8.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::hstack()
                .auto_id()
                .gap(8.0)
                .size((Sizing::FILL, Sizing::Hug))
                .child_align(Align::CENTER)
                .show(ui, |ui| {
                    Text::new("Alloc-free pinning fixture")
                        .id_salt("title")
                        .style(TextStyle::default().with_font_size(18.0))
                        .show(ui);
                    Frame::new()
                        .id_salt("title-spacer")
                        .size((Sizing::FILL, Sizing::Fixed(1.0)))
                        .show(ui);
                    for i in 0..3 {
                        Button::new().id_salt(("act", i)).label("Action").show(ui);
                    }
                });

            for i in 0..32 {
                Panel::hstack()
                    .id_salt(("row", i))
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt(("avatar", i))
                            .size((Sizing::Fixed(28.0), Sizing::Fixed(28.0)))
                            .show(ui);
                        Panel::vstack()
                            .id_salt(("col", i))
                            .gap(2.0)
                            .size((Sizing::FILL, Sizing::Hug))
                            .show(ui, |ui| {
                                Text::new("name")
                                    .id_salt(("name", i))
                                    .style(TextStyle::default().with_font_size(12.0))
                                    .show(ui);
                                Text::new(
                                    "longer message body that should wrap inside the Fill column",
                                )
                                .id_salt(("body", i))
                                .style(TextStyle::default().with_font_size(13.0))
                                .wrapping()
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui);
                            });
                    });
            }

            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::Fixed(28.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("footer-bg")
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui);
                    Panel::hstack()
                        .auto_id()
                        .padding(4.0)
                        .justify(Justify::Center)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Text::new("Ready")
                                .id_salt("status")
                                .style(TextStyle::default().with_font_size(11.0))
                                .show(ui);
                        });
                });
        });
}

fn main() {
    let want_dump = std::env::var("DHAT_DUMP").ok().as_deref() == Some("1");
    let _profiler = if want_dump {
        Some(dhat::Profiler::new_heap())
    } else {
        Some(dhat::Profiler::builder().testing().build())
    };

    let display = Display::from_physical(PHYSICAL, SCALE);
    let mut ui = new_ui();

    for _ in 0..WARMUP_FRAMES {
        black_box(ui.frame(
            FrameStamp::new(display, std::time::Duration::ZERO),
            &mut (),
            build_ui,
        ));
    }
    let before = dhat::HeapStats::get();
    for _ in 0..MEASURE_FRAMES {
        black_box(ui.frame(
            FrameStamp::new(display, std::time::Duration::ZERO),
            &mut (),
            build_ui,
        ));
    }
    let after = dhat::HeapStats::get();

    let block_delta = after.total_blocks - before.total_blocks;
    let byte_delta = after.total_bytes - before.total_bytes;

    println!(
        "alloc_free: warmup={WARMUP_FRAMES} measure={MEASURE_FRAMES} \
         ({PHYSICAL:?} @ {SCALE}x)"
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
        eprintln!("  DHAT_DUMP=1 cargo bench --bench alloc_free");
        eprintln!("  open dhat-heap.json at https://nnethercote.github.io/dh_view/");
        std::process::exit(1);
    }

    println!();
    println!("PASS: palantir CPU pipeline is allocation-free in steady state.");
}
