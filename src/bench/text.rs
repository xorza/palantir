use crate::common::content_hash::ContentHash;
use crate::common::hash::hash_str;
use crate::layout::types::align::HAlign;
use crate::primitives::widget_id::WidgetId;
use crate::text::{FontFamily, FontWeight, LineFit, MeasureResult, ShapeParams, TextShaper};
use criterion::{BatchSize, Criterion};
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

fn measure_truncated_width(
    shaper: &TextShaper,
    wid: WidgetId,
    text: &str,
    params: ShapeParams,
) -> MeasureResult {
    let text_hash = hash_str(text);
    let hash = ContentHash(text_hash);
    shaper.shape_unbounded(wid, 0, hash, text, text_hash, params);
    let target = params
        .max_width_px
        .expect("truncation benchmark requires a finite width");
    shaper.shape_wrap(
        wid,
        0,
        hash,
        text,
        params,
        (target.max(0.0) * 64.0).round() as u32,
        LineFit::Ellipsis,
    )
}

pub fn bench(c: &mut Criterion) {
    let wid = WidgetId::from_hash("text-shape-width-churn");
    c.bench_function("text_shape/ellipsis_width_churn", |b| {
        b.iter_batched(
            || {
                let shaper = TextShaper::with_bundled_fonts();
                measure_truncated_width(&shaper, wid, TEXT, params(39.75));
                shaper
            },
            |shaper| {
                for i in 0..WIDTHS_PER_BATCH {
                    let measured =
                        measure_truncated_width(&shaper, wid, TEXT, params(40.0 + i as f32 * 0.25));
                    black_box(measured.size);
                }
            },
            BatchSize::SmallInput,
        );
    });
}
