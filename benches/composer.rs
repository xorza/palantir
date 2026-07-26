use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::composer);
criterion_main!(benches);
