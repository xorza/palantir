use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::composer);
criterion_main!(benches);
