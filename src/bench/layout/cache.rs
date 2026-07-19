//! Cache-effectiveness A/B benchmark. Measures the **measure cache**
//! (the only cache left in the layout pipeline) under representative
//! and adversarial workload shapes — a light list (`measure/*`, mono
//! text fallback), a heavier
//! stencil-clipped variant with real cosmic-text shaping (`heavy/*`),
//! and deep (`deep/*`) / broad (`broad/*`) trees — in up to four arms:
//!
//! - `cached`: warm-up frame primes the cache; subsequent iterations
//!   hit at the highest stable subtree root every frame (in steady
//!   state, the root itself).
//! - `forced_miss`: warm-up primes the cache; each iteration then calls
//!   `Ui::clear_measure_cache()` before recording, so measure rebuilds
//!   from scratch.
//! - `resizing`: rotates four viewport widths so `available_q` misses
//!   at the root while unchanged branches remain eligible for reuse.
//! - `localized`: broad-tree only; toggles one leaf's paint hash while
//!   keeping layout stable so unchanged sibling-subtree hits stay visible.
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
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};
use std::hint::black_box;
use std::time::Duration;

const GROUPS: usize = 100;
const ROWS_PER_GROUP: usize = 10;

const HEAVY_GROUPS: usize = 50;
const HEAVY_ROWS_PER_GROUP: usize = 8;

const DEEP_DEPTH: usize = 192;
const BROAD_FANOUT: usize = 8;
const BROAD_DEPTH: usize = 3;

fn build(ui: &mut Ui) {
    Panel::vstack()
        .id_salt("nested-root")
        .gap(4.0)
        .padding(8.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for g in 0..GROUPS {
                Panel::vstack()
                    .id_salt(("group", g))
                    .gap(2.0)
                    .padding(4.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Text::new("Group header")
                            .id_salt(("g-hdr", g))
                            .style(&TextStyle::default().with_font_size(14.0))
                            .show(ui);
                        for r in 0..ROWS_PER_GROUP {
                            Panel::hstack()
                                .id_salt(("row", g, r))
                                .gap(6.0)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui, |ui| {
                                    Frame::new()
                                        .id_salt(("avatar", g, r))
                                        .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                                        .show(ui);
                                    Text::new("row name")
                                        .id_salt(("name", g, r))
                                        .style(&TextStyle::default().with_font_size(12.0))
                                        .show(ui);
                                    Text::new("meta info")
                                        .id_salt(("meta", g, r))
                                        .style(&TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                        Frame::new()
                            .id_salt(("g-ftr", g))
                            .size((Sizing::FILL, Sizing::fixed(2.0)))
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
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for g in 0..HEAVY_GROUPS {
                Panel::vstack()
                    .id_salt(("h-group", g))
                    .gap(4.0)
                    .padding(8.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .background(group_bg.clone())
                    .clip_rounded()
                    .show(ui, |ui| {
                        Text::new("Group header — interesting copy that wraps")
                            .id_salt(("h-g-hdr", g))
                            .style(&TextStyle::default().with_font_size(15.0))
                            .show(ui);
                        for r in 0..HEAVY_ROWS_PER_GROUP {
                            Panel::hstack()
                                .id_salt(("h-row", g, r))
                                .gap(8.0)
                                .padding(6.0)
                                .size((Sizing::FILL, Sizing::HUG))
                                .background(row_bg.clone())
                                .clip_rounded()
                                .show(ui, |ui| {
                                    // Inner zstack adds a nesting level — exercises
                                    // measure on a deeper tree.
                                    Panel::zstack()
                                        .id_salt(("h-avatar-wrap", g, r))
                                        .size((Sizing::fixed(24.0), Sizing::fixed(24.0)))
                                        .show(ui, |ui| {
                                            Frame::new()
                                                .id_salt(("h-avatar", g, r))
                                                .size((Sizing::FILL, Sizing::FILL))
                                                .background(avatar_bg.clone())
                                                .show(ui);
                                        });
                                    Text::new("row name with longer text content")
                                        .id_salt(("h-name", g, r))
                                        .style(&TextStyle::default().with_font_size(13.0))
                                        .show(ui);
                                    Text::new("meta info — secondary detail")
                                        .id_salt(("h-meta", g, r))
                                        .style(&TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                    });
            }
        });
}

fn build_deep(ui: &mut Ui) {
    build_deep_level(ui, 0);
}

fn build_deep_level(ui: &mut Ui, depth: usize) {
    if depth == DEEP_DEPTH {
        Frame::new()
            .id_salt("deep-leaf")
            .size((Sizing::FILL, Sizing::fixed(1.0)))
            .show(ui);
        return;
    }

    Panel::vstack()
        .id_salt(("deep", depth))
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| build_deep_level(ui, depth + 1));
}

fn build_broad(ui: &mut Ui) {
    build_broad_variant(ui, false);
}

fn build_broad_variant(ui: &mut Ui, changed: bool) {
    build_broad_level(ui, 0, 0, changed);
}

fn build_broad_level(ui: &mut Ui, depth: usize, key: usize, changed: bool) {
    Panel::vstack()
        .id_salt(("broad", depth, key))
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            if depth == BROAD_DEPTH {
                Frame::new()
                    .id_salt(("broad-leaf", key))
                    .size((Sizing::FILL, Sizing::fixed(1.0)))
                    .background(Background {
                        fill: if changed && key == 0 {
                            Color::rgb(0.5, 0.25, 0.75).into()
                        } else {
                            Color::TRANSPARENT.into()
                        },
                        ..Default::default()
                    })
                    .show(ui);
                return;
            }

            for child in 0..BROAD_FANOUT {
                build_broad_level(ui, depth + 1, key * BROAD_FANOUT + child, changed);
            }
        });
}

fn bench_cache_pair(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    display: Display,
    make_ui: fn() -> Ui,
    build: fn(&mut Ui),
) {
    group.bench_function(format!("{name}/cached"), |b| {
        let mut ui = make_ui();
        let _ = ui.record(FrameStamp::new(display, Duration::ZERO), build);
        b.iter(|| {
            black_box(ui.record(FrameStamp::new(display, Duration::ZERO), build));
        });
    });

    group.bench_function(format!("{name}/forced_miss"), |b| {
        let mut ui = make_ui();
        let _ = ui.record(FrameStamp::new(display, Duration::ZERO), build);
        b.iter(|| {
            ui.clear_measure_cache();
            black_box(ui.record(FrameStamp::new(display, Duration::ZERO), build));
        });
    });
}

fn bench_cache_workload(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    display: Display,
    build: fn(&mut Ui),
) {
    bench_cache_pair(group, name, display, Ui::for_test, build);

    let resize_displays = [1280, 1248, 1216, 1184]
        .map(|width| Display::from_physical(glam::UVec2::new(width, 800), 2.0));
    group.bench_function(format!("{name}/resizing"), |b| {
        let mut ui = Ui::for_test();
        let _ = ui.record(FrameStamp::new(resize_displays[0], Duration::ZERO), build);
        let mut frame = 0usize;
        b.iter(|| {
            frame = (frame + 1) % resize_displays.len();
            black_box(ui.record(
                FrameStamp::new(resize_displays[frame], Duration::ZERO),
                build,
            ));
        });
    });
}

fn bench_broad_localized(group: &mut BenchmarkGroup<'_, WallTime>, name: &str, display: Display) {
    group.bench_function(format!("{name}/localized"), |b| {
        let mut ui = Ui::for_test();
        let _ = ui.record(FrameStamp::new(display, Duration::ZERO), |ui| {
            build_broad_variant(ui, false);
        });
        let mut changed = false;
        b.iter(|| {
            changed = !changed;
            black_box(ui.record(FrameStamp::new(display, Duration::ZERO), |ui| {
                build_broad_variant(ui, changed);
            }));
        });
    });
}

pub fn bench(c: &mut Criterion) {
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut group = c.benchmark_group("caches");

    bench_cache_pair(&mut group, "measure", display, Ui::for_test, build);
    bench_cache_pair(
        &mut group,
        "heavy/measure",
        display,
        Ui::for_test_text,
        build_heavy,
    );

    bench_cache_workload(&mut group, "deep/measure", display, build_deep);
    bench_cache_workload(&mut group, "broad/measure", display, build_broad);
    bench_broad_localized(&mut group, "broad/measure", display);

    group.finish();
}

#[cfg(test)]
mod tests {
    use crate::display::Display;
    use crate::forest::layer::Layer;
    use crate::ui::Ui;
    use crate::ui::frame::FrameStamp;
    use std::time::Duration;

    use crate::bench::layout::cache::{
        BROAD_DEPTH, BROAD_FANOUT, DEEP_DEPTH, build_broad, build_broad_variant, build_deep,
    };

    fn cold_frame(build: fn(&mut Ui)) -> Ui {
        let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
        let mut ui = Ui::for_test();
        let _ = ui.record(FrameStamp::new(display, Duration::ZERO), build);
        ui
    }

    #[test]
    fn adversarial_workloads_pin_tree_shape_and_overlap_cost() {
        let deep = cold_frame(build_deep);
        let deep_nodes = DEEP_DEPTH + 2;
        assert_eq!(
            deep.forest.trees[Layer::Main].records.len(),
            deep_nodes,
            "viewport + {DEEP_DEPTH} nested panels + leaf",
        );
        let expected_deep_snapshots = deep_nodes + (2..deep_nodes).sum::<usize>();
        assert_eq!(
            deep.layout_engine.cache.nodes.live, expected_deep_snapshots,
            "overlapping deep snapshots expose quadratic retained rows",
        );

        let broad = cold_frame(build_broad);
        let panel_count = (0..=BROAD_DEPTH)
            .map(|depth| BROAD_FANOUT.pow(depth as u32))
            .sum::<usize>();
        let leaf_count = BROAD_FANOUT.pow(BROAD_DEPTH as u32);
        assert_eq!(
            broad.forest.trees[Layer::Main].records.len(),
            1 + panel_count + leaf_count,
            "viewport + balanced panels + one leaf per terminal panel",
        );
        let terminal_subtree = 1 + 1;
        let depth_two_subtree = 1 + BROAD_FANOUT * terminal_subtree;
        let depth_one_subtree = 1 + BROAD_FANOUT * depth_two_subtree;
        let root_subtree = 1 + BROAD_FANOUT * depth_one_subtree;
        let expected_broad_snapshots = (1 + root_subtree)
            + root_subtree
            + BROAD_FANOUT * depth_one_subtree
            + BROAD_FANOUT.pow(2) * depth_two_subtree
            + BROAD_FANOUT.pow(3) * terminal_subtree;
        assert_eq!(
            broad.layout_engine.cache.nodes.live, expected_broad_snapshots,
            "balanced overlap cost is O(N log N) for this exact fixture",
        );
    }

    #[test]
    fn localized_change_hits_unchanged_sibling_subtrees() {
        let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
        let mut ui = Ui::for_test();
        let _ = ui.record(FrameStamp::new(display, Duration::ZERO), |ui| {
            build_broad_variant(ui, false);
        });
        let _ = ui.record(FrameStamp::new(display, Duration::ZERO), |ui| {
            build_broad_variant(ui, true);
        });
        assert_eq!(
            ui.layout_engine.scratch.cache_hits.len(),
            21,
            "seven unchanged siblings hit at each of the three branch levels",
        );
    }
}
