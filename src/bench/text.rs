use crate::common::content_hash::ContentHash;
use crate::common::hash::hash_str;
use crate::layout::types::align::HAlign;
use crate::primitives::widget_id::WidgetId;
use crate::scene::record_store::RecordStore;
use crate::text::{
    FontFamily, FontWeight, LineFit, ShapeParams, TextMeasurement, TextReuseCache, TextRunIdentity,
    TextShaper,
};
use criterion::{BatchSize, Criterion};
use std::hint::black_box;

const TEXT: &str = "A long property label used to exercise character-precise truncation across many previously unseen widths.";
const WIDTHS_PER_BATCH: u32 = 256;

#[derive(Debug)]
struct BenchState {
    shaper: TextShaper,
    reuse: TextReuseCache,
}

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
    reuse: &mut TextReuseCache,
    wid: WidgetId,
    text: &str,
    params: ShapeParams,
) -> TextMeasurement {
    let text_hash = hash_str(text);
    let hash = ContentHash(text_hash);
    let identity = TextRunIdentity {
        widget_id: wid,
        ordinal: 0,
        authoring_hash: hash,
    };
    let prepared = reuse
        .prepare_run(shaper, identity, text, text_hash, params)
        .unwrap();
    let target = params
        .max_width_px
        .expect("truncation benchmark requires a finite width");
    prepared
        .shape_bounded(target, params.halign, LineFit::Ellipsis)
        .unwrap()
}

pub fn bench(c: &mut Criterion) {
    let store = RecordStore::default();
    let arena_text = store.intern_str(TEXT);
    c.bench_function("text_input/arena_clone_drop", |b| {
        b.iter(|| black_box(arena_text.clone()));
    });

    let wid = WidgetId::from_hash("text-shape-width-churn");
    c.bench_function("text_shape/ellipsis_width_churn", |b| {
        b.iter_batched(
            || {
                let shaper = TextShaper::with_bundled_fonts();
                let mut reuse = TextReuseCache::default();
                measure_truncated_width(&shaper, &mut reuse, wid, TEXT, params(39.75));
                BenchState { shaper, reuse }
            },
            |mut state| {
                for i in 0..WIDTHS_PER_BATCH {
                    let measured = measure_truncated_width(
                        &state.shaper,
                        &mut state.reuse,
                        wid,
                        TEXT,
                        params(40.0 + i as f32 * 0.25),
                    );
                    black_box(measured.size);
                }
            },
            BatchSize::SmallInput,
        );
    });
}
