//! Phase-1 measure-cache bench. Builds a leaf-heavy static tree and
//! benches `end_frame()` two ways:
//!
//! - `cached`: every frame after the first hits the cache for every
//!   leaf — the steady state we're optimizing for.
//! - `forced_miss`: clears the cache between frames via the
//!   `internals::clear_measure_cache` helper, so every frame
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
use palantir::Display;
use palantir::{Configure, Frame, Panel, Sizing, Text, Ui};
use std::hint::black_box;

const TEXT_LEAVES: usize = 500;
const FRAME_LEAVES: usize = 500;

// `nested` workload: GROUPS × (header + ROWS_PER_GROUP × (avatar + 2 text)
// + footer). Default constants give 100 groups × (1 + 30 + 1) = 3 200 nodes
// at depth 4 — the kind of static deeply-nested panel where Phase 1 still
// pays per-leaf recursion + per-leaf cache lookup, and Phase 2 (subtree
// skip) would cut the whole subtree at one of the upper levels.
const GROUPS: usize = 100;
const ROWS_PER_GROUP: usize = 10;

/// Build the bench UI: one VStack root with `TEXT_LEAVES` static text
/// rows and `FRAME_LEAVES` plain frames. All ids are stable across
/// frames (call-site keys + loop index), so subtree hashes match
/// frame-to-frame.
fn build_flat(ui: &mut Ui) {
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

/// Deeply-nested static workload: a sidebar of `GROUPS` collapsible-style
/// groups, each one a VStack containing a header, `ROWS_PER_GROUP` rows
/// (HStack with avatar + name + meta), and a footer. Identical inputs
/// every frame so every node is a cache hit candidate. Lets us see how
/// much of the per-leaf saving is recursion / map-lookup overhead vs.
/// actual leaf body work.
fn build_nested(ui: &mut Ui) {
    Panel::vstack_with_id("nested-root")
        .gap(4.0)
        .padding(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..GROUPS {
                Panel::vstack_with_id(("group", g))
                    .gap(2.0)
                    .padding(4.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Text::with_id(("g-hdr", g), "Group header")
                            .size_px(14.0)
                            .show(ui);
                        for r in 0..ROWS_PER_GROUP {
                            Panel::hstack_with_id(("row", g, r))
                                .gap(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    Frame::with_id(("avatar", g, r))
                                        .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                                        .show(ui);
                                    Text::with_id(("name", g, r), "row name")
                                        .size_px(12.0)
                                        .show(ui);
                                    Text::with_id(("meta", g, r), "meta info")
                                        .size_px(11.0)
                                        .show(ui);
                                });
                        }
                        Frame::with_id(("g-ftr", g))
                            .size((Sizing::FILL, Sizing::Fixed(2.0)))
                            .show(ui);
                    });
            }
        });
}

fn bench(c: &mut Criterion) {
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut group = c.benchmark_group("measure_cache");

    for (name, build) in [
        ("flat", build_flat as fn(&mut Ui)),
        ("nested", build_nested as fn(&mut Ui)),
    ] {
        group.bench_function(format!("{name}/cached"), |b| {
            let mut ui = Ui::new();
            // Warm-up: first frame populates every snapshot. Subsequent
            // criterion iterations are pure cache hits on every leaf.
            ui.begin_frame(display);
            build(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                ui.begin_frame(display);
                build(&mut ui);
                black_box(ui.end_frame());
            });
        });

        group.bench_function(format!("{name}/forced_miss"), |b| {
            let mut ui = Ui::new();
            b.iter(|| {
                palantir::internals::clear_measure_cache(&mut ui);
                ui.begin_frame(display);
                build(&mut ui);
                black_box(ui.end_frame());
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
