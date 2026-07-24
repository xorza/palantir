use crate::layout::types::align::HAlign;
use crate::primitives::widget_id::WidgetId;
use crate::scene::record_store::RecordStore;
use crate::text::system::{TextRunSlot, TextSystem};
use crate::text::wrap::{LineFit, TextWrap};
use crate::text::{FontFamily, FontWeight, TextMeasurement, TextShapeRequest, TextShaper};
use criterion::{BatchSize, Criterion};
use std::hint::black_box;

const TEXT: &str = "A long property label used to exercise character-precise truncation across many previously unseen widths.";
const WIDTHS_PER_BATCH: u32 = 256;

/// Distinct labels per frame in the reuse-layer A/B benches — a
/// realistic mid-size UI's worth of text runs, enough that both maps
/// see real cache pressure rather than one L1-resident entry.
const REUSE_LAYER_LABELS: usize = 64;

#[derive(Debug)]
struct BenchState {
    text: TextSystem,
}

fn measure_truncated_width(
    text_system: &mut TextSystem,
    slot: TextRunSlot,
    text: &str,
    width: f32,
) -> TextMeasurement {
    let request =
        TextShapeRequest::unbounded(text, 14.0, 16.8, FontFamily::Sans, FontWeight::Regular);
    text_system.measure(slot, request, TextWrap::Ellipsis, HAlign::Left, Some(width))
}

/// A/B for the `TextSystem` reuse-slot layer: steady-state
/// `TextSystem::measure` hits vs the raw shaper dispatches the
/// layer-less design would issue per frame — one unbounded probe for
/// single-line runs; unbounded root + bounded resolve for wrapped
/// runs. Each iteration measures all [`REUSE_LAYER_LABELS`] labels
/// once (one "frame"); request construction — including the text
/// hash — is inside the loop on both sides, as layout rebuilds it
/// each frame either way.
fn bench_reuse_layer(c: &mut Criterion) {
    let labels: Vec<String> = (0..REUSE_LAYER_LABELS)
        .map(|i| format!("Reuse layer probe label number {i}"))
        .collect();
    let slots: Vec<TextRunSlot> = (0..REUSE_LAYER_LABELS)
        .map(|i| TextRunSlot {
            widget_id: WidgetId::from_hash("reuse-layer-bench"),
            ordinal: i as u16,
        })
        .collect();
    fn request_for(text: &str) -> TextShapeRequest<'_> {
        TextShapeRequest::unbounded(text, 14.0, 16.8, FontFamily::Sans, FontWeight::Regular)
    }
    const WRAP_W: f32 = 150.0;

    c.bench_function("text_shape/reuse_layer/single_line_hit_x64", |b| {
        let mut text_system = TextSystem::new(TextShaper::new());
        for (slot, text) in slots.iter().zip(&labels) {
            text_system.measure(
                *slot,
                request_for(text),
                TextWrap::SingleLine,
                HAlign::Left,
                None,
            );
        }
        b.iter(|| {
            for (slot, text) in slots.iter().zip(&labels) {
                black_box(text_system.measure(
                    *slot,
                    request_for(text),
                    TextWrap::SingleLine,
                    HAlign::Left,
                    None,
                ));
            }
        });
    });

    c.bench_function("text_shape/reuse_layer/single_line_dispatch_x64", |b| {
        let shaper = TextShaper::new();
        for text in &labels {
            shaper.dispatch(request_for(text));
        }
        b.iter(|| {
            for text in &labels {
                black_box(shaper.dispatch(request_for(text)));
            }
        });
    });

    c.bench_function("text_shape/reuse_layer/wrap_hit_x64", |b| {
        let mut text_system = TextSystem::new(TextShaper::new());
        for (slot, text) in slots.iter().zip(&labels) {
            text_system.measure(
                *slot,
                request_for(text),
                TextWrap::Wrap,
                HAlign::Left,
                Some(WRAP_W),
            );
        }
        b.iter(|| {
            for (slot, text) in slots.iter().zip(&labels) {
                black_box(text_system.measure(
                    *slot,
                    request_for(text),
                    TextWrap::Wrap,
                    HAlign::Left,
                    Some(WRAP_W),
                ));
            }
        });
    });

    c.bench_function("text_shape/reuse_layer/wrap_dispatch_x64", |b| {
        let shaper = TextShaper::new();
        for text in &labels {
            let request = request_for(text);
            shaper.dispatch(request);
            shaper.dispatch(request.bounded(WRAP_W, HAlign::Left, LineFit::Wrap));
        }
        b.iter(|| {
            for text in &labels {
                let request = request_for(text);
                black_box(shaper.dispatch(request));
                black_box(shaper.dispatch(request.bounded(WRAP_W, HAlign::Left, LineFit::Wrap)));
            }
        });
    });
}

pub fn bench(c: &mut Criterion) {
    let store = RecordStore::default();
    let arena_text = store.intern_str(TEXT);
    c.bench_function("text_input/arena_clone_drop", |b| {
        b.iter(|| black_box(arena_text.clone()));
    });

    let reuse_slot = TextRunSlot {
        widget_id: WidgetId::from_hash("text-shape-reuse-hit"),
        ordinal: 0,
    };
    c.bench_function("text_shape/ellipsis_reuse_hit", |b| {
        let mut text_system = TextSystem::new(TextShaper::new());
        measure_truncated_width(&mut text_system, reuse_slot, TEXT, 80.0);
        b.iter(|| {
            black_box(measure_truncated_width(
                &mut text_system,
                reuse_slot,
                TEXT,
                80.0,
            ));
        });
    });

    bench_reuse_layer(c);

    let churn_slot = TextRunSlot {
        widget_id: WidgetId::from_hash("text-shape-width-churn"),
        ordinal: 0,
    };
    c.bench_function("text_shape/ellipsis_width_churn", |b| {
        b.iter_batched(
            || {
                let mut text = TextSystem::new(TextShaper::new());
                measure_truncated_width(&mut text, churn_slot, TEXT, 39.75);
                BenchState { text }
            },
            |mut state| {
                for i in 0..WIDTHS_PER_BATCH {
                    let measured = measure_truncated_width(
                        &mut state.text,
                        churn_slot,
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
