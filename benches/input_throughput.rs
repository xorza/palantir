use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::input);
criterion_main!(benches);
