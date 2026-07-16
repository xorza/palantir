use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::text_atlas);
criterion_main!(benches);
