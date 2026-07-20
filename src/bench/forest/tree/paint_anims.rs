use crate::forest::tree::paint_anims::{PaintAnim, PaintAnimEntry, PaintAnims};
use criterion::{Criterion, Throughput};
use std::hint::black_box;
use std::time::Duration;

const SHAPE_COUNT: u32 = 65_536;
const NOW: Duration = Duration::from_millis(250);

fn last_shape_registry() -> PaintAnims {
    let mut anims = PaintAnims::default();
    anims.push_entry(
        SHAPE_COUNT - 1,
        PaintAnimEntry {
            anim: PaintAnim::BlinkOpacity {
                half_period: Duration::from_millis(500),
                started_at: Duration::ZERO,
            },
            row: 0,
            node_idx: 0,
        },
    );
    anims
}

pub fn bench(c: &mut Criterion) {
    let anims = last_shape_registry();
    assert_eq!(anims.shape_indices, [SHAPE_COUNT - 1]);

    let mut group = c.benchmark_group("paint_anims");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.throughput(Throughput::Elements(SHAPE_COUNT as u64));
    group.bench_function("sequential_last_shape", |b| {
        b.iter(|| {
            let mut cursor = anims.cursor();
            for shape_idx in 0..SHAPE_COUNT {
                black_box(cursor.sample(shape_idx, NOW));
            }
        });
    });
    group.finish();
}
