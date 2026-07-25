use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::text_grid);
criterion_main!(benches);
