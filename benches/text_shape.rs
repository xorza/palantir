use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::text_shape);
criterion_main!(benches);
