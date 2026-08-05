use crate::display::Display;
use crate::frame_fixture::{BENCH_SCALE, FrameFixture, build_ui};
use crate::input::sense::Sense;
use crate::primitives::rect::Rect;
use crate::primitives::transform::TranslateScale;
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
const QUERY: Vec2 = Vec2::new(640.0, 400.0);
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

fn fixture(density: Density) -> Cascade {
    let interactive_count = ENTRY_COUNT * density.percent / 100;
    let mut cascade = Cascade::default();
    cascade.entries.reserve(ENTRY_COUNT);
    for index in 0..ENTRY_COUNT {
        // Put inert rows above interactive rows so sparse traversal cost stays visible.
        let interactive = index < interactive_count;
        if interactive {
            cascade.hits.push(HitRow {
                rect: Rect::new(0.0, 0.0, 1280.0, 800.0),
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
        build_ui(&mut state, BENCH_SCALE, ui);
    });
    h
}

pub(crate) fn bench(c: &mut Criterion, _: crate::bench::Arms) {
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
        group.bench_function(BenchmarkId::new("targets", density.label), |b| {
            b.iter(|| {
                black_box(cascade.hit_test_targets(
                    QUERY,
                    Sense::hovers,
                    Sense::scrolls,
                    Sense::pinches,
                ))
            });
        });
        group.bench_function(BenchmarkId::new("click_focus", density.label), |b| {
            b.iter(|| {
                black_box(cascade.hit_test(QUERY, Sense::clicks));
                black_box(cascade.hit_test_focusable(QUERY));
            });
        });
    }

    group.finish();
}
