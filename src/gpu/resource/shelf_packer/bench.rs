//! The shelf packer's benches, on a 4096² side of glyph-sized rectangles, 6 to 37 texels wide and
//! 8 to 39 tall, as a raster atlas holds them.
//!
//! - `atomic/shelf_packer/cpu/churn/<live>`: one rectangle freed and one of another size placed,
//!   with `live` placed, as an atlas that evicts to make room does. Per pair.
//! - `atomic/shelf_packer/cpu/miss/<live>`: a rectangle no free span holds, refused, with `live`
//!   placed and every other row claimed by full shelves of its height: the whole search an atlas
//!   pays before each eviction. Per refusal.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::gpu::resource::shelf_packer::ShelfPacker;
use criterion::{Criterion, Throughput};
use glam::U16Vec2;
use std::hint::black_box;

const SIDE: u16 = 4096;
const LIVE: [usize; 2] = [1000, 8000];

/// The `i`th rectangle's size, from a fixed hash of `i`.
const fn size_of(i: u32) -> U16Vec2 {
    let hash = i.wrapping_mul(2_654_435_761);
    U16Vec2::new(6 + (hash >> 27) as u16, 8 + ((hash >> 22) & 31) as u16)
}

/// A packer holding `live` rectangles after `4 · live` rounds of churn, so its free spans are as
/// fragmented as a steady atlas's; the placed rectangles, and the next round.
fn churned(live: usize) -> (ShelfPacker, Vec<(U16Vec2, u16)>, u32) {
    let mut packer = ShelfPacker::new(SIDE);
    let mut placed = Vec::with_capacity(live);
    let mut next = 0;
    for _ in 0..live {
        let size = size_of(next);
        next += 1;
        let origin = packer
            .allocate(size)
            .expect("the side holds every rectangle");
        placed.push((origin, size.x));
    }
    for _ in 0..4 * live {
        churn(&mut packer, &mut placed, next);
        next += 1;
    }
    (packer, placed, next)
}

/// Round `round` of churn: free a rectangle of `placed` picked by a fixed hash of `round`, as an
/// atlas evicts what was drawn least lately, and place one of the round's size in its stead.
fn churn(packer: &mut ShelfPacker, placed: &mut [(U16Vec2, u16)], round: u32) {
    let at = (round.wrapping_mul(0x9E37_79B9) >> 7) as usize % placed.len();
    let (origin, width) = placed[at];
    packer.deallocate(origin, width);
    let size = size_of(round);
    let origin = packer
        .allocate(size)
        .expect("a side a quarter full places every rectangle");
    placed[at] = (origin, size.x);
}

pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "shelf_packer", Scale::Sample);
    group.throughput(Throughput::Elements(1));
    for live in LIVE {
        group.bench_function(Backend::Cpu.case(&format!("churn/{live}")), |bencher| {
            let (mut packer, mut placed, mut next) = churned(live);
            bencher.iter(|| {
                churn(&mut packer, &mut placed, next);
                next = next.wrapping_add(1);
            });
        });
        group.bench_function(Backend::Cpu.case(&format!("miss/{live}")), |bencher| {
            let (mut packer, _, _) = churned(live);
            let too_wide = U16Vec2::new(SIDE, 20);
            // Whole rows of its height, until no row is unclaimed and no shelf holds it whole.
            while packer.allocate(too_wide).is_some() {}
            bencher.iter(|| black_box(packer.allocate(black_box(too_wide))));
        });
    }
}
