use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::curve_pipeline);
criterion_main!(benches);
