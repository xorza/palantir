use criterion::{criterion_group, criterion_main};
use palantir::bench;

criterion_group! {
    name = benches;
    config = bench::frame_config();
    targets = bench::frame
}
criterion_main!(benches);
