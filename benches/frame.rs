use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group! {
    name = benches;
    config = bench::frame_config();
    targets = bench::frame
}
criterion_main!(benches);
