use crate::display::Display;
use crate::frame_fixture::{BENCH_SCALE, FrameFixture};
use crate::input::sense::Sense;
use crate::primitives::rect::Rect;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;
use crate::scene::cascade::engine::CascadeEngine;
use crate::scene::cascade::entry::{EntryRow, HitRow};
use crate::ui::harness::UiHarness;
use criterion::{BenchmarkId, Criterion};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use std::time::Duration;

const ENTRY_COUNT: usize = 8192;
/// Tile pitch for the hit fixture's disjoint rects, and how many fit a
/// row. `TILE` leaves a 2 px gutter so a query can land between tiles.
const TILE: f32 = 20.0;
const TILES_PER_ROW: usize = 64;
/// In a gutter: inside the tiled region's bounds but no rect contains
/// it, so the scan runs to the end and finds nothing. The full-traversal
/// case, and the one a spatial index would fix.
const QUERY_MISS: Vec2 = Vec2::new(TILE - 1.0, TILE - 1.0);

/// Inside the *last-pushed* interactive tile — the top of the paint
/// order, so `hits_under`'s reverse scan matches on its first test.
/// Pairs with [`QUERY_MISS`]: flat against density where the miss is
/// linear, which is what makes the sweep a scan-length curve rather
/// than two copies of the same number.
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

const DENSITIES: [Density; 4] = [
    Density {
        label: "0_percent",
        percent: 0,
    },
    Density {
        label: "1_percent",
        percent: 1,
    },
    Density {
        label: "10_percent",
        percent: 10,
    },
    Density {
        label: "100_percent",
        percent: 100,
    },
];

/// `density.percent` of [`ENTRY_COUNT`] rows are interactive, tiled into
/// a disjoint grid.
///
/// **Disjoint is the whole point.** Every row used to carry the same
/// full-screen rect, so `hits_under`'s reverse scan matched the first
/// row it tested and returned — at every density, for every query. The
/// group read 1-2 ns across the board and measured an early exit rather
/// than the traversal it is named for. Only interactive rows reach
/// `Cascade::hits` at all, so the old comment's "inert rows above
/// interactive ones" described rows that were never there.
///
/// With tiles, a query lands in at most one, and the two `QUERY_*`
/// constants pick how far the scan runs before it stops.
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
        engine.run(&first.ui.forest, &first.ui.layout, display, &mut cascade);
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
            &source.ui.forest,
            &source.ui.layout,
            self.display,
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
            &source.ui.forest,
            &source.ui.layout,
            self.display,
            &mut self.cascade,
        );
        self.use_second = !self.use_second;
    }
}

fn record_fixture(mut state: FrameFixture) -> UiHarness {
    let mut h = UiHarness::with_text(FRAME_SIZE).scale(DISPLAY_SCALE);
    let _ = h.frame(|ui| {
        state.render(BENCH_SCALE, ui);
    });
    h
}

pub(crate) fn bench(c: &mut Criterion, _: crate::bench::Run<'_>) {
    let mut group = c.benchmark_group("cascade/run");
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(4));

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

    let mut group = c.benchmark_group("cascade/hit_test");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));

    for density in DENSITIES {
        let cascade = fixture(density);
        let interactive = ENTRY_COUNT * density.percent / 100;
        // `topmost` exits on the first row tested and should stay flat
        // across the sweep; `miss` traverses every row and should scale
        // with it. The gap between the two curves is the scan cost a
        // spatial index would remove — and the reason a single query
        // could not measure this group.
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
