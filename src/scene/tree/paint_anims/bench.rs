//! The paint-animation cursor's walk over a frame's shapes when only the last
//! one animates: the scan every shape pays to learn it is still, at a count
//! far past a real frame's so a per-shape regression shows.

use crate::bench::fixture::{Backend, Kind, Scale};
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::{PaintAnimEntry, PaintAnims};
use crate::shape::paint_animation::curves;
use crate::shape::paint_animation::{PaintAnimation, PaintRepeat};
use criterion::{Criterion, Throughput};
use std::hint::black_box;
use std::time::Duration;

const SHAPE_COUNT: u32 = 65_536;
const NOW: Duration = Duration::from_millis(250);

fn last_shape_registry() -> PaintAnims {
    let mut anims = PaintAnims::default();
    anims.push_entry(PaintAnimEntry {
        anim: PaintAnimation::alpha(0.0, 1.0)
            .with_period(Duration::from_secs(1))
            .with_steps(2)
            .with_repeat(PaintRepeat::Settle(Duration::MAX))
            .with_curve(curves::square),
        shape_idx: SHAPE_COUNT - 1,
        row: 0,
        node: NodeId(0),
    });
    anims
}

/// `atomic/paint_anims/cpu/sequential_last_shape`: the cursor sampled at every shape when
/// only the last animates. Per shape.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "paint_anims", Scale::Sample);
    group.throughput(Throughput::Elements(u64::from(SHAPE_COUNT)));
    group.bench_function(Backend::Cpu.case("sequential_last_shape"), |b| {
        let anims = last_shape_registry();
        assert_eq!(anims.entries[0].shape_idx, SHAPE_COUNT - 1);
        b.iter(|| {
            let mut cursor = anims.cursor();
            for shape_idx in 0..SHAPE_COUNT {
                black_box(cursor.sample(shape_idx, NOW));
            }
        });
    });
    group.finish();
}
