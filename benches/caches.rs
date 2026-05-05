//! Cache-effectiveness A/B benchmark. One nested workload, three axes
//! (measure / encode / compose), each axis benched in two arms:
//!
//! - `cached`: warm-up frame primes every cache; subsequent iterations hit
//!   the cache at the highest stable subtree root every frame (in steady
//!   state, the root itself).
//! - `forced_miss`: warm-up primes the *other two* caches; each iteration
//!   then clears only the cache for this axis before `end_frame`, so the
//!   axis under test rebuilds from scratch while the other two stay pure
//!   cache hits.
//!
//! Ratio of `cached / forced_miss` quantifies the cache's contribution on
//! a comparable workload across all three axes. See
//! `src/layout/measure-cache.md`,
//! `src/renderer/frontend/encoder/encode-cache.md`,
//! `src/renderer/frontend/composer/compose-cache.md`.
//!
//! Gated behind the `bench-deep` feature so default `cargo bench` runs
//! only the steady-state aggregate in `frame.rs`. Run with
//! `cargo bench --features "internals bench-deep"` to exercise these.
//!
//! `Ui::new()` leaves the cosmic shaper unset, so text measurement runs
//! through the mono fallback — same shaper-free path as `benches/frame.rs`.

use criterion::{Criterion, criterion_group, criterion_main};
use palantir::support::internals;
use palantir::{Configure, Display, Frame, Panel, Sizing, Text, Ui};
use std::hint::black_box;

const GROUPS: usize = 100;
const ROWS_PER_GROUP: usize = 10;

fn build(ui: &mut Ui) {
    Panel::vstack()
        .with_id("nested-root")
        .gap(4.0)
        .padding(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..GROUPS {
                Panel::vstack()
                    .with_id(("group", g))
                    .gap(2.0)
                    .padding(4.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Text::new("Group header")
                            .with_id(("g-hdr", g))
                            .style(palantir::TextStyle::default().with_font_size(14.0))
                            .show(ui);
                        for r in 0..ROWS_PER_GROUP {
                            Panel::hstack()
                                .with_id(("row", g, r))
                                .gap(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    Frame::new()
                                        .with_id(("avatar", g, r))
                                        .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                                        .show(ui);
                                    Text::new("row name")
                                        .with_id(("name", g, r))
                                        .style(palantir::TextStyle::default().with_font_size(12.0))
                                        .show(ui);
                                    Text::new("meta info")
                                        .with_id(("meta", g, r))
                                        .style(palantir::TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                        Frame::new()
                            .with_id(("g-ftr", g))
                            .size((Sizing::FILL, Sizing::Fixed(2.0)))
                            .show(ui);
                    });
            }
        });
}

#[derive(Copy, Clone)]
enum Axis {
    Measure,
    Encode,
    Compose,
}

impl Axis {
    fn name(self) -> &'static str {
        match self {
            Axis::Measure => "measure",
            Axis::Encode => "encode",
            Axis::Compose => "compose",
        }
    }

    fn clear(self, ui: &mut Ui) {
        match self {
            Axis::Measure => internals::clear_measure_cache(ui),
            Axis::Encode => internals::clear_encode_cache(ui),
            Axis::Compose => internals::clear_compose_cache(ui),
        }
    }
}

fn bench(c: &mut Criterion) {
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut group = c.benchmark_group("caches");

    for axis in [Axis::Measure, Axis::Encode, Axis::Compose] {
        group.bench_function(format!("{}/cached", axis.name()), |b| {
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

        group.bench_function(format!("{}/forced_miss", axis.name()), |b| {
            let mut ui = Ui::new();
            ui.begin_frame(display);
            build(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                axis.clear(&mut ui);
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
