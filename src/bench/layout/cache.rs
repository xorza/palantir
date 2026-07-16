//! Cache-effectiveness A/B benchmark. Measures the **measure cache**
//! (the only cache left in the layout pipeline) under two workload
//! shapes — a light list (`measure/*`, mono text fallback) and a
//! heavier stencil-clipped variant with real cosmic-text shaping
//! (`heavy/*`) — each in two arms:
//!
//! - `cached`: warm-up frame primes the cache; subsequent iterations
//!   hit at the highest stable subtree root every frame (in steady
//!   state, the root itself).
//! - `forced_miss`: warm-up primes the cache; each iteration then calls
//!   `Ui::clear_measure_cache()` before recording, so measure rebuilds
//!   from scratch.
//!
//! Ratio of `cached / forced_miss` quantifies what MeasureCache buys
//! on a comparable workload. See `src/layout/measure-cache.md`. The
//! encode and compose caches were removed after their contributions
//! turned out to be < 1%.
//!
//! Requires the `internals` feature for reach-in helpers like
//! `Ui::clear_measure_cache`. Run with
//! `cargo bench --features internals --bench caches`.
//!
//! The `measure/*` arms use `Ui::for_test()` (cosmic shaper unset → mono
//! text fallback, same path as the colocated frame bench); the `heavy/*` arms
//! use `Ui::for_test_text()` so text-shaping cost is in the measurement.

use crate::display::Display;
use crate::forest::element::Configure;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::ui::Ui;
use crate::ui::frame::FrameStamp;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;
use criterion::Criterion;
use std::hint::black_box;
use std::time::Duration;

const GROUPS: usize = 100;
const ROWS_PER_GROUP: usize = 10;

const HEAVY_GROUPS: usize = 50;
const HEAVY_ROWS_PER_GROUP: usize = 8;

fn build(ui: &mut Ui) {
    Panel::vstack()
        .id_salt("nested-root")
        .gap(4.0)
        .padding(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..GROUPS {
                Panel::vstack()
                    .id_salt(("group", g))
                    .gap(2.0)
                    .padding(4.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Text::new("Group header")
                            .id_salt(("g-hdr", g))
                            .style(TextStyle::default().with_font_size(14.0))
                            .show(ui);
                        for r in 0..ROWS_PER_GROUP {
                            Panel::hstack()
                                .id_salt(("row", g, r))
                                .gap(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    Frame::new()
                                        .id_salt(("avatar", g, r))
                                        .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                                        .show(ui);
                                    Text::new("row name")
                                        .id_salt(("name", g, r))
                                        .style(TextStyle::default().with_font_size(12.0))
                                        .show(ui);
                                    Text::new("meta info")
                                        .id_salt(("meta", g, r))
                                        .style(TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                        Frame::new()
                            .id_salt(("g-ftr", g))
                            .size((Sizing::FILL, Sizing::Fixed(2.0)))
                            .show(ui);
                    });
            }
        });
}

/// Heavier measure-cache baseline: rounded-stencil clips on every group
/// and row, real cosmic-text shaping (no mono fallback), an extra
/// zstack layer per row for deeper nesting, and a stroke on each group
/// surface. Text shaping + deeper trees make measure genuinely
/// expensive here, so the `cached / forced_miss` ratio reflects a
/// shaping-bound workload rather than the mono-fallback `build` one.
fn build_heavy(ui: &mut Ui) {
    let group_bg = Background {
        fill: Color::hex(0x1a1a1a).into(),
        stroke: Stroke::solid(Color::hex(0x4d5663), 1.5),
        corners: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let row_bg = Background {
        fill: Color::hex(0x252525).into(),
        stroke: Stroke::ZERO,
        corners: Corners::all(6.0),
        shadow: Shadow::NONE,
    };
    let avatar_bg = Background {
        fill: Color::hex(0x3a4a5c).into(),
        stroke: Stroke::ZERO,
        corners: Corners::all(10.0),
        shadow: Shadow::NONE,
    };
    Panel::vstack()
        .id_salt("heavy-root")
        .gap(6.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..HEAVY_GROUPS {
                Panel::vstack()
                    .id_salt(("h-group", g))
                    .gap(4.0)
                    .padding(8.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .background(group_bg.clone())
                    .clip_rounded()
                    .show(ui, |ui| {
                        Text::new("Group header — interesting copy that wraps")
                            .id_salt(("h-g-hdr", g))
                            .style(TextStyle::default().with_font_size(15.0))
                            .show(ui);
                        for r in 0..HEAVY_ROWS_PER_GROUP {
                            Panel::hstack()
                                .id_salt(("h-row", g, r))
                                .gap(8.0)
                                .padding(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .background(row_bg.clone())
                                .clip_rounded()
                                .show(ui, |ui| {
                                    // Inner zstack adds a nesting level — exercises
                                    // measure on a deeper tree.
                                    Panel::zstack()
                                        .id_salt(("h-avatar-wrap", g, r))
                                        .size((Sizing::Fixed(24.0), Sizing::Fixed(24.0)))
                                        .show(ui, |ui| {
                                            Frame::new()
                                                .id_salt(("h-avatar", g, r))
                                                .size((Sizing::FILL, Sizing::FILL))
                                                .background(avatar_bg.clone())
                                                .show(ui);
                                        });
                                    Text::new("row name with longer text content")
                                        .id_salt(("h-name", g, r))
                                        .style(TextStyle::default().with_font_size(13.0))
                                        .show(ui);
                                    Text::new("meta info — secondary detail")
                                        .id_salt(("h-meta", g, r))
                                        .style(TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                    });
            }
        });
}

pub fn bench(c: &mut Criterion) {
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut group = c.benchmark_group("caches");

    group.bench_function("measure/cached", |b| {
        let mut ui = Ui::for_test();
        let _ = ui.frame(FrameStamp::new(display, Duration::ZERO), build);
        b.iter(|| {
            black_box(ui.frame(FrameStamp::new(display, Duration::ZERO), build));
        });
    });

    group.bench_function("measure/forced_miss", |b| {
        let mut ui = Ui::for_test();
        let _ = ui.frame(FrameStamp::new(display, Duration::ZERO), build);
        b.iter(|| {
            ui.clear_measure_cache();
            black_box(ui.frame(FrameStamp::new(display, Duration::ZERO), build));
        });
    });

    // Heavy-workload variant: rounded-stencil clips on every group +
    // row, real cosmic-text shaping, deeper nesting, strokes. Heavier
    // baseline for the measure cache.
    group.bench_function("heavy/measure/cached", |b| {
        let mut ui = Ui::for_test_text();
        let _ = ui.frame(FrameStamp::new(display, Duration::ZERO), build_heavy);
        b.iter(|| {
            black_box(ui.frame(FrameStamp::new(display, Duration::ZERO), build_heavy));
        });
    });

    group.bench_function("heavy/measure/forced_miss", |b| {
        let mut ui = Ui::for_test_text();
        let _ = ui.frame(FrameStamp::new(display, Duration::ZERO), build_heavy);
        b.iter(|| {
            ui.clear_measure_cache();
            black_box(ui.frame(FrameStamp::new(display, Duration::ZERO), build_heavy));
        });
    });

    group.finish();
}
