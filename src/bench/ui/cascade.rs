use crate::input::sense::Sense;
use crate::primitives::rect::Rect;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::{Cascades, EntryRow};
use criterion::{BenchmarkId, Criterion};
use glam::Vec2;
use std::hint::black_box;
use std::time::Duration;

const ENTRY_COUNT: usize = 8192;
const QUERY: Vec2 = Vec2::new(640.0, 400.0);

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

fn fixture(density: Density) -> Cascades {
    let interactive_count = ENTRY_COUNT * density.percent / 100;
    let mut cascades = Cascades::default();
    cascades.entries.reserve(ENTRY_COUNT);
    for index in 0..ENTRY_COUNT {
        // Put inert rows above interactive rows so sparse traversal cost stays visible.
        let interactive = index < interactive_count;
        if interactive {
            cascades.hit_entries.push(index as u32);
        }
        cascades.entries.push(EntryRow {
            widget_id: WidgetId::from_hash(index),
            rect: Rect::new(0.0, 0.0, 1280.0, 800.0),
            sense: if interactive {
                Sense::HOVER | Sense::CLICK | Sense::SCROLL | Sense::PINCH
            } else {
                Sense::NONE
            },
            focusable: interactive,
            disabled: false,
            layout_rect: Rect::new(0.0, 0.0, 1280.0, 800.0),
            transform: TranslateScale::IDENTITY,
        });
    }
    cascades
}

pub fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("cascade/hit_test");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));

    for density in DENSITIES {
        let cascades = fixture(density);
        group.bench_function(BenchmarkId::new("targets", density.label), |b| {
            b.iter(|| {
                black_box(cascades.hit_test_targets(
                    QUERY,
                    Sense::hovers,
                    Sense::scrolls,
                    Sense::pinches,
                ))
            });
        });
        group.bench_function(BenchmarkId::new("click_focus", density.label), |b| {
            b.iter(|| {
                black_box(cascades.hit_test(QUERY, Sense::clicks));
                black_box(cascades.hit_test_focusable(QUERY));
            });
        });
    }

    group.finish();
}
