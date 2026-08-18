use crate::layout::ShapedText;
use crate::layout::types::align::HAlign;
use crate::primitives::widget_id::WidgetId;
use crate::text::cosmic;
use crate::text::key::WrapBound;
use crate::text::request::TextShapeRequest;
use crate::text::request::internals::TestShape;
use crate::text::shaper::TextShaper;
use crate::text::system::{TextRunSlot, TextSystem};
use crate::text::wrap::{LineFit, TextWrap, WrapFloor};
use crate::text::{FontFamily, FontWeight};
use criterion::Criterion;
use rustc_hash::FxHashSet;
use std::hint::black_box;

const TEXT: &str = "A long property label used to exercise character-precise truncation across many previously unseen widths.";

/// Distinct committed widths a drag frame cycles through before
/// repeating. Comfortably past [`cosmic::PROTECTED_KEEP_FRAMES`] so a
/// recycled width is a genuine miss rather than an accidental hit —
/// otherwise the arm would quietly drift into measuring the cache.
const DRAG_WIDTHS: u32 = 512;

/// Drag frames run before the measured section, to decide residency
/// deterministically rather than leaving it to however many iterations
/// criterion picked. Past [`cosmic::PROTECTED_KEEP_FRAMES`], so a
/// promoted buffer would still be inside its window and unretired
/// widths would pile up visibly; under [`DRAG_WIDTHS`], so no width
/// repeats.
const DRAG_PRIME_FRAMES: u32 = 256;

/// Shaped buffers a superseding drag may hold: the live width, the
/// unbounded root, and whatever is still inside the probation window,
/// with room to spare. Derived rather than hardcoded so it tracks the
/// window it is really about — the failure it guards against retains
/// [`cosmic::PROTECTED_KEEP_FRAMES`] of them instead.
const DRAG_RESIDENCY_LIMIT: usize = cosmic::PROBATION_KEEP_FRAMES as usize * 2 + 4;

/// Distinct labels per frame in the reuse-layer A/B benches — a
/// realistic mid-size UI's worth of text runs, enough that both maps
/// see real cache pressure rather than one L1-resident entry.
const REUSE_LAYER_LABELS: usize = 64;

/// The face every arm shapes in, stated once. `TestShape` is the same
/// fixture the in-tree tests describe a face with — `bench` implies
/// `internals`, so this side gets it too rather than re-deriving the
/// constants per helper.
/// Leading as a multiple of the font size. Named because two faces here
/// want the same proportion — [`UI_FACE`] and whatever size the
/// interleaving arm asks [`measure_truncated_face`] for — and a ratio
/// spelled twice is one that can be changed in one of them.
const LEADING_RATIO: f32 = 1.2;

const UI_FACE: TestShape = TestShape {
    font_size_px: 14.0,
    line_height_px: 14.0 * LEADING_RATIO,
    max_width_px: None,
    family: FontFamily::Sans,
    weight: FontWeight::Regular,
    halign: HAlign::Auto,
};

fn measure_truncated_width(
    text_system: &mut TextSystem,
    slot: TextRunSlot,
    text: &str,
    width: f32,
) -> ShapedText {
    let request = UI_FACE.unbounded_request(text);
    text_system.measure(slot, request, TextWrap::Ellipsis, HAlign::Left, Some(width))
}

/// [`measure_truncated_width`] at a caller-chosen face, for the arm that
/// interleaves them.
fn measure_truncated_face(
    text_system: &mut TextSystem,
    slot: TextRunSlot,
    text: &str,
    width: f32,
    font_size_px: f32,
    weight: FontWeight,
) -> ShapedText {
    let request = TestShape {
        font_size_px,
        line_height_px: font_size_px * LEADING_RATIO,
        weight,
        ..UI_FACE
    }
    .unbounded_request(text);
    text_system.measure(slot, request, TextWrap::Ellipsis, HAlign::Left, Some(width))
}

/// A/B for the `TextSystem` reuse-slot layer: steady-state
/// `TextSystem::measure` hits vs the raw shaper dispatches the
/// layer-less design would issue per frame — one unbounded probe for
/// single-line runs; unbounded root + bounded resolve for wrapped
/// runs. Each iteration measures all [`REUSE_LAYER_LABELS`] labels
/// once and then ends the frame; request construction — including the
/// text hash — is inside the loop on both sides, as layout rebuilds it
/// each frame either way.
///
/// The frame boundary belongs in the measured section because the reuse
/// layer's cost is not only its per-run lookup:
/// `TextSystem::end_full_record`
/// retains every row it holds, once a frame, and the layer-less arms
/// have no rows to retain. Leaving it out hands the layer a discount on
/// the very comparison meant to justify it.
fn bench_reuse_layer(c: &mut Criterion) {
    bench_shared_content(c);

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
        UI_FACE.unbounded_request(text)
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
            text_system.end_full_record(&FxHashSet::default());
        });
    });

    c.bench_function("text_shape/reuse_layer/single_line_dispatch_x64", |b| {
        let shaper = TextShaper::new();
        for text in &labels {
            shaper.shape(request_for(text), WrapFloor::Skip);
        }
        b.iter(|| {
            for text in &labels {
                black_box(shaper.shape(request_for(text), WrapFloor::Skip));
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
            text_system.end_full_record(&FxHashSet::default());
        });
    });

    c.bench_function("text_shape/reuse_layer/wrap_dispatch_x64", |b| {
        let shaper = TextShaper::new();
        for text in &labels {
            let request = request_for(text);
            shaper.shape(request, WrapFloor::Skip);
            shaper.shape(
                request.with_bound(WrapBound::new(WRAP_W, HAlign::Left, LineFit::Wrap)),
                WrapFloor::Skip,
            );
        }
        b.iter(|| {
            for text in &labels {
                let request = request_for(text);
                black_box(shaper.shape(request, WrapFloor::Skip));
                black_box(shaper.shape(
                    request.with_bound(WrapBound::new(WRAP_W, HAlign::Left, LineFit::Wrap)),
                    WrapFloor::Skip,
                ));
            }
        });
    });
}

/// The two workloads that separate a per-widget reuse address from a
/// content-addressed one, kept as the standing evidence for why the reuse
/// map is keyed on `(WidgetId, ordinal)` rather than on `TextShapeKey`.
///
/// `shared_content` draws one repeated label — a table column of "Enabled" —
/// across many widgets, where content addressing would collapse 64 rows into
/// one. `contended_width` measures that same repeated label at two widths,
/// which per-widget rows hold in separate wrap slots and a single shared row
/// cannot. Content-keying was prototyped against both: it lost 46% and 170%
/// respectively, and 37-43% on the plain hit paths, because a 24-byte
/// `TextShapeKey` hash costs more than the row dedup saves.
fn bench_shared_content(c: &mut Criterion) {
    const REPEATED: &str = "Enabled";
    const WRAP_W: f32 = 150.0;
    let slots: Vec<TextRunSlot> = (0..REUSE_LAYER_LABELS)
        .map(|i| TextRunSlot {
            widget_id: WidgetId::from_hash("shared-content-bench"),
            ordinal: i as u16,
        })
        .collect();
    fn request() -> TextShapeRequest<'static> {
        UI_FACE.unbounded_request(REPEATED)
    }

    c.bench_function("text_shape/reuse_layer/shared_content_x64", |b| {
        let mut text_system = TextSystem::new(TextShaper::new());
        for slot in &slots {
            text_system.measure(*slot, request(), TextWrap::Wrap, HAlign::Left, Some(WRAP_W));
        }
        b.iter(|| {
            for slot in &slots {
                black_box(text_system.measure(
                    *slot,
                    request(),
                    TextWrap::Wrap,
                    HAlign::Left,
                    Some(WRAP_W),
                ));
            }
            text_system.end_full_record(&FxHashSet::default());
        });
    });

    c.bench_function("text_shape/reuse_layer/contended_width_x64", |b| {
        let mut text_system = TextSystem::new(TextShaper::new());
        let widths = [WRAP_W, WRAP_W - 40.0];
        for (i, slot) in slots.iter().enumerate() {
            text_system.measure(
                *slot,
                request(),
                TextWrap::Wrap,
                HAlign::Left,
                Some(widths[i % 2]),
            );
        }
        b.iter(|| {
            for (i, slot) in slots.iter().enumerate() {
                black_box(text_system.measure(
                    *slot,
                    request(),
                    TextWrap::Wrap,
                    HAlign::Left,
                    Some(widths[i % 2]),
                ));
            }
            text_system.end_full_record(&FxHashSet::default());
        });
    });
}

/// One frame of a resize drag, which is the workload the shaped-buffer
/// cache's retention policy exists for and the only arm here that
/// reaches it: every other arm holds its widths still or never crosses
/// a frame boundary, so probation, protection, supersession and expiry
/// are all unreachable from them.
///
/// A drag commits a new whole-pixel width every frame, so each frame
/// mints a bounded key nothing can ask for again — but leaves the
/// *unbounded* root untouched, which is why one long-lived
/// `TextSystem` is the honest fixture. Rebuilding it per batch (what
/// the neighbouring churn arm does, to guarantee cold misses) would
/// measure a cold cache forever and hide the retention behaviour
/// entirely.
///
/// Both halves of a frame are modelled — layout's measure *and* the
/// encoder's restore — because the restore is what promotes a buffer
/// onto the long window. A layout-only fixture leaves everything
/// probationary, ages it out regardless, and reports a bounded cache
/// whether retention works or not.
///
/// The residency assertion is the standing guard: with supersession
/// working the drag holds a handful of buffers; without it every
/// rendered frame is promoted to the 120-frame window and residency
/// tracks the drag's length instead.
fn bench_resize_drag(c: &mut Criterion) {
    let slot = TextRunSlot {
        widget_id: WidgetId::from_hash("text-shape-resize-drag"),
        ordinal: 0,
    };
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());

    // Prime a fixed-length drag and judge residency on that. Reading it
    // after the measured section instead would make the guard depend on
    // however many iterations criterion chose — and report nothing at
    // all under `--list`, where the routine never runs.
    for step in 0..DRAG_PRIME_FRAMES {
        black_box(drag_frame(&mut text, &shaper, slot, step));
    }
    let resident = shaper.cosmic_cache_len();
    eprintln!(
        "[text_shape] resize_drag_frame resident_buffers={resident} \
         after {DRAG_PRIME_FRAMES} frames",
    );
    assert!(
        resident <= DRAG_RESIDENCY_LIMIT,
        "a {DRAG_PRIME_FRAMES}-frame drag settled at {resident} shaped \
         buffers, over the {DRAG_RESIDENCY_LIMIT} the probation window \
         allows — supersession is not reaching the bounded keys it mints, \
         so retention is tracking the protected window",
    );

    let mut step = DRAG_PRIME_FRAMES;
    c.bench_function("text_shape/resize_drag_frame", |b| {
        b.iter(|| {
            let measured = drag_frame(&mut text, &shaper, slot, step);
            step = step.wrapping_add(1);
            black_box(measured.measured)
        });
    });
}

/// One drag frame end to end: layout commits a fresh width, the encoder
/// restores the buffer it will replay, and the frame boundary ages the
/// cache.
///
/// All three matter. Drop the render and every buffer stays
/// probationary, ages out on its own, and the arm reports a bounded
/// cache whether retention works or not; drop the frame boundary and a
/// drag becomes an unbounded fill.
fn drag_frame(
    text: &mut TextSystem,
    shaper: &TextShaper,
    slot: TextRunSlot,
    step: u32,
) -> ShapedText {
    let width = 40.0 + (step % DRAG_WIDTHS) as f32 * 0.25;
    let measured = measure_truncated_width(text, slot, TEXT, width);
    shaper.render_ensure(TextShapeRequest::for_key(TEXT, measured.key));
    text.end_full_record(&FxHashSet::default());
    measured
}

/// The truncation *miss* path, which is where the ellipsis-advance memo
/// is consulted: every frame commits a fresh width, so the cut is redone
/// and the "…" reservation asked for again.
///
/// Two arms, because the memo's whole question is how many faces a frame
/// interleaves. `one_face` is the easy case any memo handles. `two_faces`
/// alternates a body style with a heavier heading — a header above its
/// detail row, a tree sized per depth — which is the order record
/// traversal actually produces, and which a single-slot memo misses on
/// every call.
///
/// The label is long enough that the cut keeps a short prefix: that is
/// the asymmetry `measure_truncated` is built around, since the whole
/// string is shaped once into the cached unbounded probe and only the
/// prefix is reshaped per width.
fn bench_ellipsis_churn(c: &mut Criterion) {
    const HEADING_PX: f32 = 20.0;
    let shaper = TextShaper::new();
    let mut group = c.benchmark_group("text_shape/ellipsis_width_churn");

    let mut text = TextSystem::new(shaper.clone());
    let slot = TextRunSlot {
        widget_id: WidgetId::from_hash("text-shape-ellipsis-churn"),
        ordinal: 0,
    };
    let mut step = 0u32;
    group.bench_function("one_face", |b| {
        b.iter(|| {
            let width = 40.0 + (step % DRAG_WIDTHS) as f32 * 0.25;
            step = step.wrapping_add(1);
            let measured = measure_truncated_width(&mut text, slot, TEXT, width);
            text.end_full_record(&FxHashSet::default());
            black_box(measured.measured)
        });
    });

    let mut text = TextSystem::new(shaper.clone());
    let slots = [
        TextRunSlot {
            widget_id: WidgetId::from_hash("text-shape-ellipsis-churn-body"),
            ordinal: 0,
        },
        TextRunSlot {
            widget_id: WidgetId::from_hash("text-shape-ellipsis-churn-head"),
            ordinal: 0,
        },
    ];
    let mut step = 0u32;
    group.bench_function("two_faces", |b| {
        b.iter(|| {
            let width = 40.0 + (step % DRAG_WIDTHS) as f32 * 0.25;
            step = step.wrapping_add(1);
            // Body then heading, the way a row records: with one memo
            // slot each of these evicts the other's face.
            let body =
                measure_truncated_face(&mut text, slots[0], TEXT, width, 14.0, FontWeight::Regular);
            let head = measure_truncated_face(
                &mut text,
                slots[1],
                TEXT,
                width,
                HEADING_PX,
                FontWeight::Bold,
            );
            text.end_full_record(&FxHashSet::default());
            black_box((body.measured, head.measured))
        });
    });
    group.finish();
}

pub(crate) fn bench(c: &mut Criterion, _: crate::bench::Run<'_>) {
    bench_reuse_layer(c);
    bench_resize_drag(c);
    bench_ellipsis_churn(c);
}
