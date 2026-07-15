use aperture::composer_bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, composer_bench::bench);
criterion_main!(benches);
