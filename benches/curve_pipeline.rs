use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::curve_pipeline);
criterion_main!(benches);
