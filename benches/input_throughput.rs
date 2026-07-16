use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::input);
criterion_main!(benches);
