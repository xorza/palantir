//! Cache-effectiveness A/B benchmark for the **measure cache**, over a light
//! list (`measure/*`, mono text), a stencil-clipped shaped-text variant
//! (`heavy/*`), deep (`deep/*`) and broad (`broad/*`) trees, a virtualized list,
//! and a Hug-column grid (`grid/intrinsic`). Arms:
//!
//! - `cached`: warm-up primes the cache; each iteration forgets the last run
//!   first, or the engine would keep its output and restore nothing.
//! - `forced_miss`: clears the layout cache before each record.
//! - `resizing`: rotates four viewport widths, so `available_q` misses at the
//!   root while unchanged branches stay reusable.
//! - `localized`: broad tree only; toggles one leaf's fill weight.
//! - `localized_height`: broad tree only; toggles one leaf's height on an
//!   overflowing surface, so siblings hit across offers.
//!
//! `cached / forced_miss` is what the cache buys on a comparable workload.
//!
//! Run with `cargo bench --features bench --bench criterion -- caches`.

#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::bench::Run;
use crate::internals::harness::UiHarness;
use crate::layout::cache::internals::{BroadChange, build_broad, build_broad_variant, build_deep};
use crate::layout::counters::PhaseTimings;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};
use std::hint::black_box;

const GROUPS: usize = 100;
const ROWS_PER_GROUP: usize = 10;

const HEAVY_GROUPS: usize = 50;
const HEAVY_ROWS_PER_GROUP: usize = 8;

const GRID_ROWS: usize = 128;

/// Frames each arm runs for its measure/arrange split report, sampled per
/// frame rather than averaged into criterion's wall-clock estimate.
const PHASE_WARMUP_FRAMES: usize = 8;
const PHASE_EVIDENCE_FRAMES: usize = 64;

/// Sorted-sample summary of one phase. Min is the signal: the upper half
/// measures machine interference.
#[derive(Clone, Copy, Debug)]
struct PhaseSummary {
    min_us: f64,
    median_us: f64,
}

fn summarize(samples: &mut [u64]) -> PhaseSummary {
    assert!(!samples.is_empty());
    samples.sort_unstable();
    PhaseSummary {
        min_us: samples[0] as f64 / 1_000.0,
        median_us: samples[samples.len() / 2] as f64 / 1_000.0,
    }
}

/// Report how one arm splits across measure and arrange.
///
/// Criterion times a whole frame, hiding that the measure cache can
/// short-circuit a whole subtree while arrange walks every node.
/// `arrange_over_measure` is the headline: on a `cached` arm, the factor by
/// which the uncached half dominates.
///
/// `step` runs one iteration and returns that frame's engine timings.
fn report_phases(label: &str, mut step: impl FnMut() -> PhaseTimings) {
    for _ in 0..PHASE_WARMUP_FRAMES {
        step();
    }
    let mut measure = Vec::with_capacity(PHASE_EVIDENCE_FRAMES);
    let mut arrange = Vec::with_capacity(PHASE_EVIDENCE_FRAMES);
    let mut capture = Vec::with_capacity(PHASE_EVIDENCE_FRAMES);
    for _ in 0..PHASE_EVIDENCE_FRAMES {
        let t = step();
        measure.push(t.measure_ns);
        arrange.push(t.arrange_ns);
        capture.push(t.capture_ns);
    }
    let cap = summarize(&mut capture);
    let m = summarize(&mut measure);
    let a = summarize(&mut arrange);
    let ratio = if m.min_us > 0.0 {
        format!("{:.1}x", a.min_us / m.min_us)
    } else {
        "n/a".to_owned()
    };
    eprintln!(
        "[caches] {label} measure_min_us={:.2} measure_median_us={:.2} \
         arrange_min_us={:.2} arrange_median_us={:.2} arrange_over_measure={ratio} \
         capture_min_us={:.2} capture_median_us={:.2} capture_share={:.0}%",
        m.min_us,
        m.median_us,
        a.min_us,
        a.median_us,
        cap.min_us,
        cap.median_us,
        100.0 * cap.min_us / (m.min_us + a.min_us + cap.min_us).max(1e-9),
    );
}

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
                                    Block::new()
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
                        Block::new()
                            .id_salt(("g-ftr", g))
                            .size((Sizing::FILL, Sizing::fixed(2.0)))
                            .show(ui);
                    });
            }
        });
}

/// Heavier baseline: rounded-stencil clips on every group and row, real
/// cosmic-text shaping, an extra zstack per row, and group strokes, so
/// measure is shaping-bound rather than mono-fallback.
fn build_heavy(ui: &mut Ui) {
    let group_bg = Background {
        fill: RgbaF32::hex(0x1a1a1a).into(),
        border: Stroke::new(RgbaF32::hex(0x4d5663), 1.5),
        corners: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let row_bg = Background {
        fill: RgbaF32::hex(0x252525).into(),
        border: Stroke::NONE,
        corners: Corners::all(6.0),
        shadow: Shadow::NONE,
    };
    let avatar_bg = Background {
        fill: RgbaF32::hex(0x3a4a5c).into(),
        border: Stroke::NONE,
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
                                    // Adds a nesting level.
                                    Panel::zstack()
                                        .id_salt(("h-avatar-wrap", g, r))
                                        .size((Sizing::fixed(24.0), Sizing::fixed(24.0)))
                                        .show(ui, |ui| {
                                            Block::new()
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

fn build_grid_intrinsics(ui: &mut Ui) {
    Grid::new()
        .id_salt("grid-intrinsic-root")
        .cols([Track::HUG, Track::HUG])
        .rows([Track::HUG; GRID_ROWS])
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for row in 0..GRID_ROWS {
                Text::new("unbreakable_identifier")
                    .id_salt(("grid-label", row))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .grid_cell((row as u16, 0))
                    .show(ui);
                Text::new("long natural-width grid value")
                    .id_salt(("grid-value", row))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .grid_cell((row as u16, 1))
                    .show(ui);
            }
        });
}

fn bench_cache_pair(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    make_ui: fn() -> UiHarness,
    build: fn(&mut Ui),
) {
    {
        let mut h = make_ui();
        report_phases(&format!("{name}/cached"), || {
            h.engines.layout.forget_last_run();
            let _ = h.frame(build);
            h.engines.layout.scratch.counters.phase_timings()
        });
    }
    group.bench_function(format!("{name}/cached"), |b| {
        let mut h = make_ui();
        let _ = h.frame(build);
        b.iter(|| {
            h.engines.layout.forget_last_run();
            black_box(h.frame(build));
        });
    });

    {
        let mut h = make_ui();
        report_phases(&format!("{name}/forced_miss"), || {
            h.engines.layout.cache.forget_all();
            let _ = h.frame(build);
            h.engines.layout.scratch.counters.phase_timings()
        });
    }
    group.bench_function(format!("{name}/forced_miss"), |b| {
        let mut h = make_ui();
        let _ = h.frame(build);
        b.iter(|| {
            h.engines.layout.cache.forget_all();
            black_box(h.frame(build));
        });
    });
}

fn bench_cache_workload(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    make_ui: fn() -> UiHarness,
    build: fn(&mut Ui),
) {
    bench_cache_pair(group, name, make_ui, build);

    let resize_widths = [1280, 1248, 1216, 1184].map(|width| glam::UVec2::new(width, 800));
    {
        let mut h = make_ui();
        let mut frame = 0usize;
        report_phases(&format!("{name}/resizing"), || {
            frame = (frame + 1) % resize_widths.len();
            let _ = h.resize(resize_widths[frame]).frame(build);
            h.engines.layout.scratch.counters.phase_timings()
        });
    }
    group.bench_function(format!("{name}/resizing"), |b| {
        let mut h = make_ui();
        let _ = h.resize(resize_widths[0]).frame(build);
        let mut frame = 0usize;
        b.iter(|| {
            frame = (frame + 1) % resize_widths.len();
            black_box(h.resize(resize_widths[frame]).frame(build));
        });
    });
}

fn bench_broad_localized(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    harness: impl Fn() -> UiHarness,
    change: BroadChange,
) {
    let toggled = |changed: bool| changed.then_some(change);
    {
        let mut h = harness();
        let mut changed = false;
        report_phases(name, || {
            changed = !changed;
            let _ = h.frame(|ui| build_broad_variant(ui, toggled(changed)));
            h.engines.layout.scratch.counters.phase_timings()
        });
    }
    group.bench_function(name, |b| {
        let mut h = harness();
        let _ = h.frame(|ui| build_broad_variant(ui, None));
        let mut changed = false;
        b.iter(|| {
            changed = !changed;
            black_box(h.frame(|ui| build_broad_variant(ui, toggled(changed))));
        });
    });
}

/// Rows in the virtualized-list arms; enough that a per-descriptor rebuild
/// shows against frame noise.
const SCROLL_ROWS: usize = 96;

/// One frame of a virtualized list showing rows `first .. first + ROWS`. The
/// window slides one row per frame, so the set of `WidgetId`s changes while
/// the count doesn't.
fn build_scroll_window(ui: &mut Ui, first: usize) {
    Panel::vstack()
        .id_salt("scroll-root")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for row in first..first + SCROLL_ROWS {
                Panel::hstack()
                    .id_salt(row)
                    .size((Sizing::FILL, Sizing::fixed(18.0)))
                    .show(ui, |_ui| {});
            }
        });
}

/// The virtualized-list path: what a scroll costs the measure cache vs the
/// same tree standing still.
///
/// `MeasureSnapshot::refresh_snapshots` reuses its `WidgetId` map only while
/// the descriptor id sequence is unchanged, so a scrolling window rebuilds it
/// every frame (one hash insert per descriptor). The arms differ only in
/// whether the window moves; rebuild counts are reported for both.
fn bench_virtual_scroll(group: &mut BenchmarkGroup<'_, WallTime>) {
    let make = || UiHarness::new(glam::UVec2::new(1280, 800)).scale(2.0);

    for (name, stride) in [("static", 0usize), ("scrolling", 1)] {
        const FRAMES: usize = 64;

        let mut h = make();
        let mut first = 0usize;
        for _ in 0..8 {
            let _ = h.frame(|ui| build_scroll_window(ui, first));
            first += stride;
        }
        let before = h.engines.layout.cache.snapshot_rebuilds.count();
        for _ in 0..FRAMES {
            let _ = h.frame(|ui| build_scroll_window(ui, first));
            first += stride;
        }
        eprintln!(
            "[caches] virtual_scroll/{name}: {} snapshot rebuilds over {FRAMES} frames \
             ({SCROLL_ROWS} rows)",
            h.engines.layout.cache.snapshot_rebuilds.count() - before,
        );

        group.bench_function(format!("virtual_scroll/{name}"), |b| {
            b.iter(|| {
                let r = h.frame(|ui| build_scroll_window(ui, first));
                first += stride;
                black_box(r)
            });
        });
    }
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.group(c);

    bench_cache_pair(
        &mut group,
        "measure",
        || UiHarness::new(glam::UVec2::new(1280, 800)).scale(2.0),
        build,
    );
    bench_cache_pair(
        &mut group,
        "heavy/measure",
        || UiHarness::with_text(glam::UVec2::new(1280, 800)).scale(2.0),
        build_heavy,
    );

    bench_cache_workload(
        &mut group,
        "deep/measure",
        || UiHarness::new(glam::UVec2::new(1280, 800)).scale(2.0),
        build_deep,
    );
    bench_cache_workload(
        &mut group,
        "broad/measure",
        || UiHarness::new(glam::UVec2::new(1280, 800)).scale(2.0),
        build_broad,
    );
    bench_broad_localized(
        &mut group,
        "broad/measure/localized",
        || UiHarness::new(glam::UVec2::new(1280, 800)),
        BroadChange::FillWeight,
    );
    bench_broad_localized(
        &mut group,
        "broad/measure/localized_height",
        || UiHarness::new(glam::UVec2::new(1280, 800)).scale(2.0),
        BroadChange::LeafHeight,
    );
    bench_virtual_scroll(&mut group);
    bench_cache_workload(
        &mut group,
        "grid/intrinsic",
        || UiHarness::with_text(glam::UVec2::new(1280, 800)).scale(2.0),
        build_grid_intrinsics,
    );

    group.finish();
}
