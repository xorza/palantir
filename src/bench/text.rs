use crate::layout::types::align::HAlign;
use crate::primitives::widget_id::WidgetId;
use crate::scene::record_store::RecordStore;
use crate::text::wrap::TextWrap;
use crate::text::{
    FontFamily, FontWeight, TextMeasurement, TextRunIdentity, TextShapeRequest, TextShaper,
    TextSystem,
};
use criterion::{BatchSize, Criterion};
use std::hint::black_box;

const TEXT: &str = "A long property label used to exercise character-precise truncation across many previously unseen widths.";
const WIDTHS_PER_BATCH: u32 = 256;

#[derive(Debug)]
struct BenchState {
    text: TextSystem,
}

fn measure_truncated_width(
    text_system: &mut TextSystem,
    identity: TextRunIdentity,
    text: &str,
    width: f32,
) -> TextMeasurement {
    let request =
        TextShapeRequest::unbounded(text, 14.0, 16.8, FontFamily::Sans, FontWeight::Regular)
            .unwrap();
    text_system
        .shape(
            identity,
            request,
            TextWrap::Ellipsis,
            HAlign::Left,
            Some(width),
        )
        .measurement
}

pub fn bench(c: &mut Criterion) {
    let store = RecordStore::default();
    let arena_text = store.intern_str(TEXT);
    c.bench_function("text_input/arena_clone_drop", |b| {
        b.iter(|| black_box(arena_text.clone()));
    });

    let reuse_identity = TextRunIdentity {
        widget_id: WidgetId::from_hash("text-shape-reuse-hit"),
        ordinal: 0,
    };
    c.bench_function("text_shape/ellipsis_reuse_hit", |b| {
        let mut text_system = TextSystem::new(TextShaper::with_bundled_fonts());
        measure_truncated_width(&mut text_system, reuse_identity, TEXT, 80.0);
        b.iter(|| {
            black_box(measure_truncated_width(
                &mut text_system,
                reuse_identity,
                TEXT,
                80.0,
            ));
        });
    });

    let churn_identity = TextRunIdentity {
        widget_id: WidgetId::from_hash("text-shape-width-churn"),
        ordinal: 0,
    };
    c.bench_function("text_shape/ellipsis_width_churn", |b| {
        b.iter_batched(
            || {
                let mut text = TextSystem::new(TextShaper::with_bundled_fonts());
                measure_truncated_width(&mut text, churn_identity, TEXT, 39.75);
                BenchState { text }
            },
            |mut state| {
                for i in 0..WIDTHS_PER_BATCH {
                    let measured = measure_truncated_width(
                        &mut state.text,
                        churn_identity,
                        TEXT,
                        40.0 + i as f32 * 0.25,
                    );
                    black_box(measured.size);
                }
            },
            BatchSize::SmallInput,
        );
    });
}
