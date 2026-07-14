use aperture::{FontFamily, FontWeight, HAlign, ShapeParams, TextShaper, TextWrap, WidgetId};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

const TEXT: &str = "A long property label used to exercise character-precise truncation across many previously unseen widths.";
const WIDTHS_PER_BATCH: u32 = 256;

fn params(width: f32) -> ShapeParams {
    ShapeParams {
        font_size_px: 14.0,
        line_height_px: 16.8,
        max_width_px: Some(width),
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
        halign: HAlign::Left,
    }
}

fn bench_text_shape(c: &mut Criterion) {
    let wid = WidgetId::from_hash("text-shape-width-churn");
    c.bench_function("text_shape/ellipsis_width_churn", |b| {
        b.iter_batched(
            || {
                let shaper = TextShaper::with_bundled_fonts();
                shaper.measure_truncated_width_for_bench(
                    wid,
                    TEXT,
                    params(39.75),
                    TextWrap::Ellipsis,
                );
                shaper
            },
            |shaper| {
                for i in 0..WIDTHS_PER_BATCH {
                    let measured = shaper.measure_truncated_width_for_bench(
                        wid,
                        TEXT,
                        params(40.0 + i as f32 * 0.25),
                        TextWrap::Ellipsis,
                    );
                    black_box(measured.size);
                }
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_text_shape);
criterion_main!(benches);
