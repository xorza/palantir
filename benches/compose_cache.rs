//! Phase-4 compose-cache bench. Same `flat` / `nested` workloads as
//! `encode_cache.rs`, but A/B'd on the compose-cache axis with the
//! measure cache and encode cache held hot in both arms — so the
//! delta is purely attributable to `Composer` subtree-skip work, not
//! measure or encode work.
//!
//! - `cached`: warm-up frame primes every cache; subsequent
//!   iterations hit the compose cache at the highest stable subtree
//!   root every frame (in steady state, the root itself).
//! - `forced_miss`: warm-up primes the measure + encode caches; each
//!   iteration then clears *only* the compose cache via
//!   `bench_support::clear_compose_cache` before `end_frame`, so the composer
//!   rebuilds every group/quad/text from scratch while measure +
//!   encode stay pure cache hits.
//!
//! Ratio of `cached / forced_miss` quantifies the Phase-4 win on the
//! same workloads the encode-cache bench uses — directly comparable.
//! See `src/renderer/frontend/composer/compose-cache.md`.

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
    let mut group = c.benchmark_group("compose_cache");

    for (name, build) in [
        ("flat", build_flat as fn(&mut Ui)),
        ("nested", build_nested as fn(&mut Ui)),
    ] {
        group.bench_function(format!("{name}/cached"), |b| {
            let mut ui = Ui::new();
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
            ui.begin_frame(display);
            build(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                palantir::bench_support::clear_compose_cache(&mut ui);
                ui.begin_frame(display);
                build(&mut ui);
                black_box(ui.end_frame());
            });
        });

        // Compose-only micro-bench. Runs `Composer::compose` over the
        // last frame's cmd buffer with the cache hot ('cached') vs
        // cleared ('forced_miss'). Isolates the compose stage from
        // the rest of `end_frame` so the compose-cache delta is visible.
        group.bench_function(format!("{name}/compose_only/cached"), |b| {
            let mut ui = Ui::new();
            ui.begin_frame(display);
            build(&mut ui);
            let _ = ui.end_frame();
            ui.__recompose();
            b.iter(|| {
                ui.__recompose();
                black_box(());
            });
        });

        group.bench_function(format!("{name}/compose_only/forced_miss"), |b| {
            let mut ui = Ui::new();
            ui.begin_frame(display);
            build(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                palantir::bench_support::clear_compose_cache(&mut ui);
                ui.__recompose();
                black_box(());
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
