use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::record_pass);
criterion_main!(benches);
