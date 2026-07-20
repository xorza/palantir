use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::paint_anims);
criterion_main!(benches);
