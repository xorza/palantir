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
//! - `forced_miss`: warm-up primes the cache; each iteration clears
//!   `FrameEngines::layout`'s cache before recording, so measure rebuilds from
//!   scratch.
//! - `resizing`: rotates four viewport widths so `available_q` misses
//!   at the root while unchanged branches remain eligible for reuse.
//! - `localized`: broad-tree only; toggles one leaf's fill weight, which
//!   leaves its geometry alone, so unchanged sibling-subtree hits stay
//!   visible.
//! - `localized_height`: broad-tree only; toggles one leaf's height on a
//!   surface the tree overflows, so every panel above it offers its
//!   children a new minimum and sibling subtrees hit across offers.
//! - `grid/intrinsic`: a 128-row real-text property grid that isolates
//!   paired min/max-content recursion on Hug columns.
//!
//! Ratio of `cached / forced_miss` quantifies what MeasureCache buys
//! on a comparable workload. The encode and compose caches were removed
//! after their contributions turned out to be < 1%.
//!
//! Run with `cargo bench --features bench --bench criterion -- caches`.
//!
//! The `measure/*` arms use `UiHarness::new(glam::UVec2::new(1280, 800))` (cosmic shaper unset → mono
//! text fallback, same path as the colocated frame bench); the `heavy/*` arms
//! use `UiHarness::with_text(glam::UVec2::new(1280, 800))` so text-shaping cost is in the measurement.

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

/// Frames each arm runs before and during its measure/arrange split
/// report. Separate from criterion's own loop so the split is sampled
/// per frame rather than averaged into one wall-clock estimate.
const PHASE_WARMUP_FRAMES: usize = 8;
const PHASE_EVIDENCE_FRAMES: usize = 64;

/// Sorted-sample summary of one phase. Min is the signal — these arms
/// share a machine with everything else on it, so the upper half of the
/// distribution measures interference rather than layout.
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

/// Report how one arm splits across the two halves of the layout pass.
///
/// Criterion times a whole CPU frame, which hides the asymmetry this
/// benchmark exists to expose: the measure cache can short-circuit a
/// whole subtree — in steady state the root, collapsing measure to a few
/// `copy_from_slice`s — while arrange walks every node with full driver
/// dispatch no matter what. `arrange_over_measure` is the headline: on a
/// `cached` arm it is the factor by which the uncached half dominates.
///
/// `step` runs one iteration of the arm and returns the engine's timings
/// for that frame.
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

/// Heavier measure-cache baseline: rounded-stencil clips on every group
/// and row, real cosmic-text shaping (no mono fallback), an extra
/// zstack layer per row for deeper nesting, and a stroke on each group
/// surface. Text shaping + deeper trees make measure genuinely
/// expensive here, so the `cached / forced_miss` ratio reflects a
/// shaping-bound workload rather than the mono-fallback `build` one.
fn build_heavy(ui: &mut Ui) {
    let group_bg = Background {
        fill: RgbaF32::hex(0x1a1a1a).into(),
        border: Stroke::new(RgbaF32::hex(0x4d5663), 1.5),
        corners: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let row_bg = Background {
        fill: RgbaF32::hex(0x252525).into(),
        border: Stroke::ZERO,
        corners: Corners::all(6.0),
        shadow: Shadow::NONE,
    };
    let avatar_bg = Background {
        fill: RgbaF32::hex(0x3a4a5c).into(),
        border: Stroke::ZERO,
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
            let _ = h.frame(build);
            h.engines.layout.scratch.counters.phase_timings()
        });
    }
    group.bench_function(format!("{name}/cached"), |b| {
        let mut h = make_ui();
        let _ = h.frame(build);
        b.iter(|| {
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

/// Rows in the virtualized-list arms — a plausible viewport's worth, and
/// enough that a per-descriptor rebuild is visible against frame noise.
const SCROLL_ROWS: usize = 96;

/// One frame of a virtualized list showing rows `first .. first + ROWS`.
///
/// The window slides by one row per frame, so the *set* of recorded
/// `WidgetId`s changes every frame even though the count never does —
/// which is the shape that matters here, not the row content.
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

/// The virtualized-list path: what a scroll costs the measure cache
/// against what the same tree costs when it holds still.
///
/// `MeasureSnapshot::refresh_snapshots` reuses its retained `WidgetId`
/// map only while the captured descriptor id *sequence* is unchanged,
/// approximated by an ordered fold. A scrolling window changes that
/// sequence every frame, so the map is rebuilt from scratch — one hash
/// insert per descriptor — every frame the gesture lasts.
///
/// The two arms differ in nothing but whether the window moves, so the
/// gap between them is the rebuild plus whatever else a changed id set
/// costs. Rebuild counts are reported for both, because the wall-clock
/// gap alone would not say which of those two it is.
fn bench_virtual_scroll(group: &mut BenchmarkGroup<'_, WallTime>) {
    let make = || UiHarness::new(glam::UVec2::new(1280, 800)).scale(2.0);

    for (name, stride) in [("static", 0usize), ("scrolling", 1)] {
        let mut h = make();
        let mut first = 0usize;
        for _ in 0..8 {
            let _ = h.frame(|ui| build_scroll_window(ui, first));
            first += stride;
        }
        let before = h.engines.layout.cache.snapshot_rebuilds.count();
        const FRAMES: usize = 64;
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
