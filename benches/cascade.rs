use aperture::cascade_bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, cascade_bench::bench);
criterion_main!(benches);
