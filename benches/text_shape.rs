use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::text_shape);
criterion_main!(benches);
