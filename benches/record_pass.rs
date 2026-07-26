use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::record_pass);
criterion_main!(benches);
