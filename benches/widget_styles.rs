use aperture::bench;
use criterion::{criterion_group, criterion_main};

criterion_group!(benches, bench::widget_styles);
criterion_main!(benches);
