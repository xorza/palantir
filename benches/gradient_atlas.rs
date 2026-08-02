use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::gradient_atlas);
criterion_main!(benches);
