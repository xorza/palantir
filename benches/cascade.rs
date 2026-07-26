use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::cascade);
criterion_main!(benches);
