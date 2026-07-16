use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::layout_caches);
criterion_main!(benches);
