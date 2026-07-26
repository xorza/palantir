use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::layout_caches);
criterion_main!(benches);
