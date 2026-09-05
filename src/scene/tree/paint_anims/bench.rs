use crate::bench::Run;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::paint_anim::{PaintAnim, PaintRepeat};
use crate::scene::tree::paint_anims::{PaintAnimEntry, PaintAnims, curves};
use criterion::{Criterion, Throughput};
use std::hint::black_box;
use std::time::Duration;

const SHAPE_COUNT: u32 = 65_536;
const NOW: Duration = Duration::from_millis(250);

fn last_shape_registry() -> PaintAnims {
    let mut anims = PaintAnims::default();
    anims.push_entry(PaintAnimEntry {
        anim: PaintAnim::alpha(0.0, 1.0)
            .period(Duration::from_secs(1))
            .steps(2)
            .repeat(PaintRepeat::Settle(Duration::MAX))
            .curve(curves::square),
        shape_idx: SHAPE_COUNT - 1,
        row: 0,
        node: NodeId(0),
    });
    anims
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let anims = last_shape_registry();
    assert_eq!(anims.entries[0].shape_idx, SHAPE_COUNT - 1);

    let mut group = run.group(c);
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
