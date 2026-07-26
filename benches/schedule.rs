use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::schedule);
criterion_main!(benches);
