use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group!(benches, bench::paint_anims);
criterion_main!(benches);
