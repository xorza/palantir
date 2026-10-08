//! The cascade's two costs: a run over the frame fixture (incremental after a
//! paint-only or transform change, and a forced full rebuild), and the hit
//! test input runs per event against a disjoint grid of interactive rows.

use crate::bench::Run;
use crate::cascade::Cascade;
use crate::cascade::cascade_key::CascadeKey;
use crate::cascade::engine::CascadeEngine;
use crate::cascade::entry::{EntryRow, HitRow};
use crate::display::Display;
use crate::input::sense::Sense;
use crate::internals::frame_fixture::{BENCH_SCALE, FrameFixture};
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use criterion::{BenchmarkId, Criterion};
use glam::{UVec2, Vec2};
use std::hint::black_box;

const ENTRY_COUNT: usize = 8192;
/// Tile pitch for the hit fixture's disjoint rects, and how many fit a row. `TILE` leaves a 2 px gutter.
const TILE: f32 = 20.0;
const TILES_PER_ROW: usize = 64;
/// In a gutter: no rect contains it, so the scan runs to the end (full traversal).
const QUERY_MISS: Vec2 = Vec2::new(TILE - 1.0, TILE - 1.0);

/// Inside the last-pushed tile, top of paint order, so `hits_under`'s reverse scan matches first. Pairs with [`QUERY_MISS`] to give a scan-length curve.
fn topmost_query(interactive_count: usize) -> Vec2 {
    let index = interactive_count.saturating_sub(1);
    let x = (index % TILES_PER_ROW) as f32 * TILE;
    let y = (index / TILES_PER_ROW) as f32 * TILE;
    Vec2::new(x + TILE * 0.5, y + TILE * 0.5)
}
const FRAME_SIZE: UVec2 = UVec2::new(3840, 4800);
const DISPLAY_SCALE: f32 = 2.0;

#[derive(Clone, Copy, Debug)]
struct Density {
    label: &'static str,
    percent: usize,
}

/// Interactive shares of the [`ENTRY_COUNT`] rows: a busy UI, and every row.
/// None interactive scans nothing, so it is not an arm.
const DENSITIES: [Density; 2] = [
    Density {
        label: "10_percent",
        percent: 10,
    },
    Density {
        label: "100_percent",
        percent: 100,
    },
];

/// `density.percent` of [`ENTRY_COUNT`] rows are interactive, in a disjoint grid.
///
/// Disjoint matters: identical full-screen rects let `hits_under`'s reverse scan exit on the first test at every density, measuring an early exit, not the traversal.
fn fixture(density: Density) -> Cascade {
    let interactive_count = ENTRY_COUNT * density.percent / 100;
    let mut cascade = Cascade::default();
    cascade.entries.reserve(ENTRY_COUNT);
    for index in 0..ENTRY_COUNT {
        if index < interactive_count {
            let x = (index % TILES_PER_ROW) as f32 * TILE;
            let y = (index / TILES_PER_ROW) as f32 * TILE;
            cascade.hits.push(HitRow {
                rect: Rect::new(x, y, TILE - 2.0, TILE - 2.0),
                widget_id: WidgetId::from_hash(index),
                sense: Sense::HOVER | Sense::CLICK | Sense::SCROLL | Sense::PINCH,
                focusable: true,
                disabled: false,
            });
        }
        cascade.entries.push(EntryRow {
            rect: Rect::new(0.0, 0.0, 1280.0, 800.0),
            transform: TranslateScale::IDENTITY,
            disabled: false,
        });
    }
    cascade
}

#[derive(Clone, Copy, Debug)]
enum RunMutation {
    PaintOnly,
    Transform,
}

#[derive(Debug)]
struct CascadeRunFixture {
    first: UiHarness,
    second: UiHarness,
    engine: CascadeEngine,
    cascade: Cascade,
    display: Display,
    use_second: bool,
}

impl CascadeRunFixture {
    fn new(mutation: RunMutation) -> Self {
        let display = Display::from_physical(FRAME_SIZE, DISPLAY_SCALE);
        let first = record_fixture(FrameFixture::default());
        let mut second_state = FrameFixture::default();
        match mutation {
            RunMutation::PaintOnly => second_state.tick = 1,
            RunMutation::Transform => {
                second_state.scroll_offset = Vec2::new(1.5, 0.7);
            }
        }
        let second = record_fixture(second_state);
        let mut engine = CascadeEngine::default();
        let mut cascade = Cascade::default();
        engine.run(
            first.ui.forest(),
            first.ui.layout_tables(),
            display,
            &key_of(&first, display),
            &mut cascade,
        );
        Self {
            first,
            second,
            engine,
            cascade,
            display,
            use_second: true,
        }
    }

    fn run_next(&mut self) {
        let source = if self.use_second {
            &self.second
        } else {
            &self.first
        };
        self.engine.run(
            source.ui.forest(),
            source.ui.layout_tables(),
            self.display,
            &key_of(source, self.display),
            &mut self.cascade,
        );
        self.use_second = !self.use_second;
    }

    fn run_next_full(&mut self) {
        let source = if self.use_second {
            &self.second
        } else {
            &self.first
        };
        self.engine.run_full(
            source.ui.forest(),
            source.ui.layout_tables(),
            self.display,
            &key_of(source, self.display),
            &mut self.cascade,
        );
        self.use_second = !self.use_second;
    }
}

/// The key the frame builds before each run, built per run so the cost is what a frame pays.
fn key_of(h: &UiHarness, display: Display) -> CascadeKey {
    CascadeKey::new(
        h.ui.forest(),
        h.ui.layout_tables(),
        display,
        h.ui.font_epoch(),
    )
}

fn record_fixture(mut state: FrameFixture) -> UiHarness {
    let mut h = UiHarness::with_text(FRAME_SIZE).scale(DISPLAY_SCALE);
    let _ = h.frame(|ui| {
        state.render(BENCH_SCALE, ui);
    });
    h
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.subgroup(c, "run");

    for (label, mutation) in [
        ("paint_only", RunMutation::PaintOnly),
        ("transform", RunMutation::Transform),
    ] {
        let mut fixture = CascadeRunFixture::new(mutation);
        group.bench_function(label, |b| {
            b.iter(|| {
                fixture.run_next();
                black_box(&fixture.cascade);
            });
        });
    }
    let mut run_fixture = CascadeRunFixture::new(RunMutation::Transform);
    group.bench_function("full_rebuild", |b| {
        b.iter(|| {
            run_fixture.run_next_full();
            black_box(&run_fixture.cascade);
        });
    });
    group.finish();

    let mut group = run.subgroup(c, "hit_test");

    for density in DENSITIES {
        let cascade = fixture(density);
        let interactive = ENTRY_COUNT * density.percent / 100;
        // `topmost` exits on the first row and stays flat; `miss` traverses every row and scales. The gap is the scan cost a spatial index would remove.
        for (query_label, query) in [
            ("topmost", topmost_query(interactive)),
            ("miss", QUERY_MISS),
        ] {
            let id =
                |name: &str| BenchmarkId::new(name, format!("{}/{}", query_label, density.label));
            group.bench_function(id("targets"), |b| {
                b.iter(|| black_box(cascade.hit_test_targets(black_box(query))));
            });
            group.bench_function(id("click_focus"), |b| {
                b.iter(|| black_box(cascade.hit_test_press(black_box(query))));
            });
        }
    }

    group.finish();
}
