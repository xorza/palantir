//! Phase-3 encode-cache bench. Same `flat` / `nested` workloads as
//! `measure_cache.rs`, but A/B'd on the encode-cache axis with the
//! measure cache held hot in both arms — so the delta is purely
//! attributable to `Encoder` subtree-skip work, not measure work.
//!
//! - `cached`: warm-up frame primes both caches; subsequent iterations
//!   hit the encode cache at the highest stable subtree root every
//!   frame (in steady state, the root itself).
//! - `forced_miss`: warm-up primes the measure cache; each iteration
//!   then clears *only* the encode cache via `__clear_encode_cache`
//!   before `end_frame`, so the encoder rebuilds every cmd from
//!   scratch while measure stays a pure cache hit.
//!
//! Ratio of `cached / forced_miss` quantifies the Phase-3 win on the
//! same workloads the measure-cache bench uses, so the two numbers
//! are directly comparable. See `docs/encode-cache.md`.
//!
//! `Ui::new()` leaves the cosmic shaper unset, so text measurement
//! runs through the mono fallback — same shaper-free path as
//! `benches/layout.rs` and `benches/measure_cache.rs`.

use criterion::{Criterion, criterion_group, criterion_main};
use palantir::Display;
use palantir::{Configure, Frame, Panel, Sizing, Text, Ui};
use std::hint::black_box;

const TEXT_LEAVES: usize = 500;
const FRAME_LEAVES: usize = 500;

const GROUPS: usize = 100;
const ROWS_PER_GROUP: usize = 10;

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
    let mut group = c.benchmark_group("encode_cache");

    for (name, build) in [
        ("flat", build_flat as fn(&mut Ui)),
        ("nested", build_nested as fn(&mut Ui)),
    ] {
        group.bench_function(format!("{name}/cached"), |b| {
            let mut ui = Ui::new();
            // Warm-up: first frame populates measure + encode caches.
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
            // Warm-up populates measure cache; we leave it hot so the
            // delta against `cached` measures only the encoder's work.
            ui.begin_frame(display);
            build(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                ui.__clear_encode_cache();
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
