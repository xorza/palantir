//! Phase-1 measure-cache bench. Builds a leaf-heavy static tree and
//! benches `end_frame()` two ways:
//!
//! - `cached`: every frame after the first hits the cache for every
//!   leaf — the steady state we're optimizing for.
//! - `forced_miss`: clears the cache between frames via the
//!   `__clear_measure_cache` doc-hidden helper, so every frame
//!   re-measures every leaf. Models a worst case where nothing
//!   carries forward.
//!
//! Ratio of `cached / forced_miss` quantifies the Phase-1 win on a
//! deliberately favorable workload. See `docs/measure-cache.md`.
//!
//! `Ui::new()` leaves the cosmic shaper unset, so text measurement
//! runs through the mono fallback — same shaper-free path as
//! `benches/layout.rs`, so the numbers are comparable.

use criterion::{Criterion, criterion_group, criterion_main};
use palantir::primitives::Display;
use palantir::{Configure, Frame, Panel, Sizing, Text, Ui};
use std::hint::black_box;

const TEXT_LEAVES: usize = 500;
const FRAME_LEAVES: usize = 500;

/// Build the bench UI: one VStack root with `TEXT_LEAVES` static text
/// rows and `FRAME_LEAVES` plain frames. All ids are stable across
/// frames (call-site keys + loop index), so subtree hashes match
/// frame-to-frame.
fn build_ui(ui: &mut Ui) {
    Panel::vstack_with_id("root")
        .gap(2.0)
        .padding(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for i in 0..TEXT_LEAVES {
                Text::with_id(("t", i), "static label text that does not change")
                    .size_px(13.0)
                    .show(ui);
            }
            for i in 0..FRAME_LEAVES {
                Frame::with_id(("f", i))
                    .size((Sizing::FILL, Sizing::Fixed(4.0)))
                    .show(ui);
            }
        });
}

fn bench(c: &mut Criterion) {
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut group = c.benchmark_group("measure_cache");

    group.bench_function("cached", |b| {
        let mut ui = Ui::new();
        // Warm-up: first frame populates every snapshot. Subsequent
        // criterion iterations are pure cache hits on every leaf.
        ui.begin_frame(display);
        build_ui(&mut ui);
        let _ = ui.end_frame();
        b.iter(|| {
            ui.begin_frame(display);
            build_ui(&mut ui);
            black_box(ui.end_frame());
        });
    });

    group.bench_function("forced_miss", |b| {
        let mut ui = Ui::new();
        b.iter(|| {
            ui.__clear_measure_cache();
            ui.begin_frame(display);
            build_ui(&mut ui);
            black_box(ui.end_frame());
        });
    });

    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
