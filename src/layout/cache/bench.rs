//! Cache-effectiveness A/B benchmark for the **measure cache**, over a list of
//! stencil-clipped groups with shaped text (`list/*`), deep (`deep/*`) and
//! broad (`broad/*`) mono trees, a virtualized list (`virtual_scroll/*`), and a
//! Hug-column grid of shaped text (`grid/*`). Arms:
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

use crate::bench::fixture::{Backend, Kind, Scale};
use crate::internals::ui_harness::UiHarness;
use crate::layout::cache::internals::{BroadChange, build_broad, build_broad_variant, build_deep};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::text::wrap::TextWrap;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};
use std::hint::black_box;

const LIST_GROUPS: usize = 50;
const LIST_ROWS_PER_GROUP: usize = 8;

const GRID_ROWS: usize = 128;

/// A list as an app draws one: rounded-stencil clips on every group and row,
/// real cosmic-text shaping, an extra zstack per row, and group strokes, so
/// measure is shaping-bound rather than mono-fallback.
fn build_list(ui: &mut Ui) {
    let group_chrome = Chrome::rounded(RgbaF32::hex(0x1a1a1a), Corners::all(12.0))
        .with_border(Stroke::new(RgbaF32::hex(0x4d5663), 1.5));
    let row_bg = Chrome::rounded(RgbaF32::hex(0x252525), Corners::all(6.0));
    let avatar_chrome = Chrome::rounded(RgbaF32::hex(0x3a4a5c), Corners::all(10.0));
    Panel::vstack()
        .id_salt("heavy-root")
        .gap(6.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for g in 0..LIST_GROUPS {
                Panel::vstack()
                    .id_salt(("h-group", g))
                    .gap(4.0)
                    .padding(8.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .chrome(group_chrome.clone())
                    .clip_rounded()
                    .show(ui, |ui| {
                        Text::new("Group header — interesting copy that wraps")
                            .id_salt(("h-g-hdr", g))
                            .style(&TextStyle::default().with_font_size(15.0))
                            .show(ui);
                        for r in 0..LIST_ROWS_PER_GROUP {
                            Panel::hstack()
                                .id_salt(("h-row", g, r))
                                .gap(8.0)
                                .padding(6.0)
                                .size((Sizing::FILL, Sizing::HUG))
                                .chrome(row_bg.clone())
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
                                                .chrome(avatar_chrome.clone())
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

const SURFACE: glam::UVec2 = glam::UVec2::new(1280, 800);

/// The surface every arm renders at, with mono-fallback text.
fn mono() -> UiHarness {
    UiHarness::new(SURFACE).scale(2.0)
}

/// [`mono`] with real cosmic shaping.
fn shaped() -> UiHarness {
    UiHarness::with_text(SURFACE).scale(2.0)
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
    group.bench_function(Backend::Cpu.case(&format!("{name}/cached")), |b| {
        let mut h = make_ui();
        let _ = h.frame(build);
        b.iter(|| {
            h.engines.layout.forget_last_run();
            black_box(h.frame(build));
        });
    });

    group.bench_function(Backend::Cpu.case(&format!("{name}/forced_miss")), |b| {
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

    let resize_widths = [1280, 1248, 1216, 1184].map(|width| glam::UVec2::new(width, SURFACE.y));
    group.bench_function(Backend::Cpu.case(&format!("{name}/resizing")), |b| {
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
    group.bench_function(Backend::Cpu.case(name), |b| {
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
/// every frame (one hash insert per descriptor) and a still one never does. The
/// arms differ only in whether the window moves, which the warm-up checks.
fn bench_virtual_scroll(group: &mut BenchmarkGroup<'_, WallTime>) {
    for (name, stride) in [("static", 0usize), ("scrolling", 1)] {
        group.bench_function(Backend::Cpu.case(&format!("virtual_scroll/{name}")), |b| {
            const FRAMES: u32 = 64;

            let mut h = mono();
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
            let rebuilds = h.engines.layout.cache.snapshot_rebuilds.count() - before;
            let expected = if stride == 0 { 0 } else { FRAMES };
            assert_eq!(
                rebuilds, expected,
                "virtual_scroll/{name} snapshot rebuilds"
            );
            b.iter(|| {
                let r = h.frame(|ui| build_scroll_window(ui, first));
                first += stride;
                black_box(r)
            });
        });
    }
}

/// `integration/measure_cache/cpu/<workload>/<arm>`: record and layout frames over each
/// workload, the cache primed, cleared, or offered rotating widths, and the broad tree with one
/// leaf toggled.
pub(crate) fn integration(c: &mut Criterion) {
    let mut group = Kind::Integration.group(c, "measure_cache", Scale::Sample);

    bench_cache_pair(&mut group, "list", shaped, build_list);
    bench_cache_workload(&mut group, "deep", mono, build_deep);
    bench_cache_workload(&mut group, "broad", mono, build_broad);
    bench_broad_localized(
        &mut group,
        "broad/localized",
        || UiHarness::new(SURFACE),
        BroadChange::FillWeight,
    );
    bench_broad_localized(
        &mut group,
        "broad/localized_height",
        mono,
        BroadChange::LeafHeight,
    );
    bench_virtual_scroll(&mut group);
    bench_cache_workload(&mut group, "grid", shaped, build_grid_intrinsics);

    group.finish();
}
