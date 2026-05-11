//! Steady-state allocation-free invariant test.
//!
//! Runs a small but realistic UI through `Ui::run_frame`, warms up so
//! retained scratch / caches stabilize, then measures heap-block delta
//! over the following frame. Asserts zero net allocation, per
//! `CLAUDE.md`: "Per-frame allocation is a real metric. Steady-state
//! must be heap-alloc-free after warmup."
//!
//! Uses `dhat` as the global allocator (10-30x overhead — never use
//! this binary for timing). Failure path prints block/byte deltas and
//! suggests `DHAT_DUMP=1` for a full per-callsite JSON, viewable in
//! `dh_view` (https://nnethercote.github.io/dh_view/dh_view.html).
//!
//! Run with: `cargo bench --bench alloc_free`
//! Verbose JSON: `DHAT_DUMP=1 cargo bench --bench alloc_free`

use palantir::{
    Align, Button, Configure, Display, Frame, Justify, Panel, Sizing, Text, TextStyle, Ui,
};
use std::hint::black_box;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const WARMUP_FRAMES: usize = 16;
// 256 measure frames so an intermittent grow-on-Nth-frame allocation
// (Vec doubling, HashMap rehash) isn't lost between two snapshots.
const MEASURE_FRAMES: usize = 256;

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
    // Profiler must outlive all measurements; it dumps the JSON on
    // Drop when `--save-dhat-heap` (default with new_heap()) is set.
    let _profiler = if want_dump {
        Some(dhat::Profiler::new_heap())
    } else {
        // Without the profiler, dhat::HeapStats still tracks blocks via
        // the global allocator (the type is itself the counter), so we
        // can diff without paying the JSON-emission cost on drop.
        Some(dhat::Profiler::builder().testing().build())
    };

    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut ui = Ui::new();

    // Warm up: cache fills, scratch buffers settle, text shaper
    // populates reuse entries.
    for _ in 0..WARMUP_FRAMES {
        black_box(ui.run_frame(display, std::time::Duration::ZERO, build_ui));
    }

    let before = dhat::HeapStats::get();

    for _ in 0..MEASURE_FRAMES {
        black_box(ui.run_frame(display, std::time::Duration::ZERO, build_ui));
    }

    let after = dhat::HeapStats::get();

    let block_delta = after.total_blocks - before.total_blocks;
    let byte_delta = after.total_bytes - before.total_bytes;
    let max_blocks_live = after.max_blocks;
    let max_bytes_live = after.max_bytes;

    println!("alloc_free: warmup={WARMUP_FRAMES} measure={MEASURE_FRAMES}");
    println!(
        "  steady-state delta: {block_delta} new blocks, {byte_delta} bytes \
         ({:.2} blocks/frame avg)",
        block_delta as f64 / MEASURE_FRAMES as f64
    );
    println!("  process peak:       {max_blocks_live} live blocks, {max_bytes_live} bytes");

    if block_delta != 0 || byte_delta != 0 {
        eprintln!();
        eprintln!("FAIL: steady-state is not allocation-free.");
        eprintln!("  Re-run with `DHAT_DUMP=1 cargo bench --bench alloc_free` to emit");
        eprintln!("  dhat-heap.json; load it at https://nnethercote.github.io/dh_view/");
        eprintln!("  to see per-call-site bytes and blocks.");
        std::process::exit(1);
    }

    println!("PASS: zero net allocations across {MEASURE_FRAMES} steady-state frame(s).");
}
