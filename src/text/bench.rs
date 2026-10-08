#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::bench::Run;
use crate::layout::text::shaped_text::ShapedText;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdSet};
use crate::primitives::layout::align::HAlign;
use crate::text::cosmic::shaped_buffer_cache;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::WrapBound;
use crate::text::request::TextShapeRequest;
use crate::text::request::internals::TestShape;
use crate::text::shaper::TextShaper;
use crate::text::system::{TextRunSlot, TextSystem};
use crate::text::wrap::{LineFit, TextWrap, WrapFloor};
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, Throughput};
use std::hint::black_box;

const TEXT: &str = "A long property label used to exercise character-precise truncation across many previously unseen widths.";

/// Distinct committed widths a drag frame cycles through. Well past
/// [`crate::text::RENDERED_RUN_KEEP_FRAMES`], so a recycled width is a real
/// miss rather than a cache hit.
const DRAG_WIDTHS: u32 = 512;

/// Drag frames run before the measured section, so residency does not depend
/// on criterion's iteration count. Past
/// [`crate::text::RENDERED_RUN_KEEP_FRAMES`] (unretired widths show), under
/// [`DRAG_WIDTHS`] (no width repeats).
const DRAG_PRIME_FRAMES: u32 = 256;

/// Shaped buffers a superseding drag may hold: the live width, the unbounded
/// root and the probation window, with room to spare. Derived so it tracks
/// the window; the failure guarded against retains
/// [`crate::text::RENDERED_RUN_KEEP_FRAMES`] of them.
const DRAG_RESIDENCY_LIMIT: usize = shaped_buffer_cache::PROBATION_KEEP_FRAMES as usize * 2 + 4;

/// Distinct labels per frame in the reuse-layer A/B benches: a mid-size UI's
/// text runs, enough for real cache pressure.
const REUSE_LAYER_LABELS: usize = 64;

/// Leading as a multiple of the font size, shared by [`UI_FACE`] and
/// [`measure_truncated`].
const LEADING_RATIO: f32 = 1.2;

/// The face every arm shapes in. `TestShape` is the in-tree tests' fixture;
/// `bench` implies `internals`.
/// The body text size, which every arm but `two_faces`' heading shapes at.
const BODY_PX: f32 = 14.0;

const UI_FACE: TestShape = TestShape::new(GlyphFont {
    size: BODY_PX,
    line_height: BODY_PX * LEADING_RATIO,
    family: FontFamily::SANS,
    weight: FontWeight::REGULAR,
    slant: FontSlant::Normal,
});

/// `text` truncated to `width` at [`UI_FACE`] resized to `font_size` and `weight`.
fn measure_truncated(
    text_system: &mut TextSystem,
    slot: TextRunSlot,
    text: &str,
    width: f32,
    font_size: f32,
    weight: FontWeight,
) -> ShapedText {
    // Set field by field: outside `cfg(test)` the fixture is `font` alone, so
    // `..UI_FACE` would update nothing.
    let mut shape = UI_FACE;
    shape.font.size = font_size;
    shape.font.line_height = font_size * LEADING_RATIO;
    shape.font.weight = weight;
    let request = shape.unbounded_request(text);
    text_system
        .measure(slot, request, TextWrap::Ellipsis, HAlign::Left, Some(width))
        .shaped
}

/// One frame boundary as `FrameCycle::run` drives it: the reuse-row sweep,
/// then the clock tick. The tick is the caller's in production too; leaving it
/// out measures a cache nothing can expire from.
fn frame_end(text: &mut TextSystem, shaper: &TextShaper) {
    text.end_frame(&WidgetIdSet::default());
    shaper.tick_frame();
}

/// A/B for the `TextSystem` reuse-slot layer: steady-state
/// `TextSystem::measure` hits vs the raw shaper dispatches the layer-less
/// design would issue per frame (one unbounded probe for single-line runs;
/// unbounded root plus bounded resolve for wrapped runs). Each iteration
/// measures all [`REUSE_LAYER_LABELS`] once and ends the frame; request
/// construction, including the text hash, is inside the loop on both sides.
///
/// The frame boundary is measured, split as each design would pay it:
/// `TextSystem::end_frame` retains every row, which the layer-less arms lack,
/// so it is the layer's alone; the clock tick ages the shaped-buffer cache in
/// both designs, so it is charged to both.
fn bench_reuse_layer(c: &mut Criterion, run: Run<'_>) {
    const WRAP_W: f32 = 150.0;

    fn request_for(text: &str) -> TextShapeRequest<'_> {
        UI_FACE.unbounded_request(text)
    }

    let mut group = run.subgroup(c, "reuse_layer");
    group.throughput(Throughput::Elements(REUSE_LAYER_LABELS as u64));
    bench_shared_content(&mut group);

    let labels: Vec<String> = (0..REUSE_LAYER_LABELS)
        .map(|i| format!("Reuse layer probe label number {i}"))
        .collect();
    let slots: Vec<TextRunSlot> = (0..REUSE_LAYER_LABELS)
        .map(|i| TextRunSlot {
            widget_id: WidgetId::from_hash("reuse-layer-bench"),
            ordinal: i as u16,
        })
        .collect();

    group.bench_function("single_line_hit", |b| {
        let shaper = TextShaper::new();
        let mut text_system = TextSystem::new(shaper.clone());
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
            frame_end(&mut text_system, &shaper);
        });
    });

    group.bench_function("single_line_dispatch", |b| {
        let shaper = TextShaper::new();
        for text in &labels {
            shaper.root(request_for(text), WrapFloor::Skip);
        }
        b.iter(|| {
            for text in &labels {
                black_box(shaper.root(request_for(text), WrapFloor::Skip));
            }
            shaper.tick_frame();
        });
    });

    group.bench_function("wrap_hit", |b| {
        let shaper = TextShaper::new();
        let mut text_system = TextSystem::new(shaper.clone());
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
            frame_end(&mut text_system, &shaper);
        });
    });

    group.bench_function("wrap_dispatch", |b| {
        let shaper = TextShaper::new();
        for text in &labels {
            let request = request_for(text);
            shaper.root(request, WrapFloor::Skip);
            shaper.resolve(request.with_bound(WrapBound::new(WRAP_W, HAlign::Left, LineFit::Wrap)));
        }
        b.iter(|| {
            for text in &labels {
                let request = request_for(text);
                black_box(shaper.root(request, WrapFloor::Skip));
                black_box(shaper.resolve(request.with_bound(WrapBound::new(
                    WRAP_W,
                    HAlign::Left,
                    LineFit::Wrap,
                ))));
            }
            shaper.tick_frame();
        });
    });
    group.finish();
}

/// The two workloads that separate a per-widget reuse address from a
/// content-addressed one: the standing evidence for keying the reuse map on
/// `(WidgetId, ordinal)` rather than `TextShapeKey`.
///
/// `shared_content` draws one repeated label across many widgets, where
/// content addressing would collapse 64 rows into one. `contended_width`
/// measures that label at two widths, which per-widget rows hold in separate
/// wrap slots and a shared row cannot. Content-keying was prototyped: it
/// lost 46% and 170% on these and 37-43% on the plain hit paths, since a
/// 24-byte `TextShapeKey` hash costs more than the dedup saves.
fn bench_shared_content(group: &mut BenchmarkGroup<'_, WallTime>) {
    fn request() -> TextShapeRequest<'static> {
        UI_FACE.unbounded_request(REPEATED)
    }

    const REPEATED: &str = "Enabled";
    const WRAP_W: f32 = 150.0;
    let slots: Vec<TextRunSlot> = (0..REUSE_LAYER_LABELS)
        .map(|i| TextRunSlot {
            widget_id: WidgetId::from_hash("shared-content-bench"),
            ordinal: i as u16,
        })
        .collect();

    group.bench_function("shared_content", |b| {
        let shaper = TextShaper::new();
        let mut text_system = TextSystem::new(shaper.clone());
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
            frame_end(&mut text_system, &shaper);
        });
    });

    group.bench_function("contended_width", |b| {
        let shaper = TextShaper::new();
        let mut text_system = TextSystem::new(shaper.clone());
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
            frame_end(&mut text_system, &shaper);
        });
    });
}

/// One frame of a resize drag: the workload the shaped-buffer cache's
/// retention policy exists for, and the only arm that reaches probation,
/// protection, supersession and expiry.
///
/// Each frame commits a new whole-pixel width, minting a bounded key nothing
/// asks for again while the unbounded root stays untouched, so one long-lived
/// `TextSystem` is the honest fixture; rebuilding it per batch would measure
/// a cold cache forever.
///
/// Both halves of a frame are modelled, layout's measure and the encoder's
/// restore, because the restore promotes a buffer onto the long window; a
/// layout-only fixture ages everything out and looks bounded regardless.
///
/// The residency assertion is the standing guard: with supersession working
/// the drag holds a handful of buffers; without it every rendered frame is
/// promoted to the 120-frame window and residency tracks the drag's length.
fn bench_resize_drag(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.group(c);
    let slot = TextRunSlot {
        widget_id: WidgetId::from_hash("text-shape-resize-drag"),
        ordinal: 0,
    };
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());

    // Prime a fixed-length drag and judge residency on that, not on
    // criterion's iteration count (which is zero under `--list`).
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
    group.bench_function("resize_drag_frame", |b| {
        b.iter(|| {
            let measured = drag_frame(&mut text, &shaper, slot, step);
            step = step.wrapping_add(1);
            black_box(measured.extent.size)
        });
    });
    group.finish();
}

/// One drag frame end to end: layout commits a fresh width, the encoder
/// restores the buffer it will replay, and the frame boundary ages the cache.
/// Dropping the render leaves everything probationary; dropping the boundary
/// makes the drag an unbounded fill.
fn drag_frame(
    text: &mut TextSystem,
    shaper: &TextShaper,
    slot: TextRunSlot,
    step: u32,
) -> ShapedText {
    let width = 40.0 + (step % DRAG_WIDTHS) as f32 * 0.25;
    let measured = measure_truncated(text, slot, TEXT, width, BODY_PX, FontWeight::REGULAR);
    shaper.render_ensure(
        TextShapeRequest::for_key(TEXT, measured.key.expect("the bench shapes through cosmic"))
            .expect("the bench fixture has text"),
    );
    frame_end(text, shaper);
    measured
}

/// The truncation miss path, where the ellipsis-advance memo is consulted:
/// every frame commits a fresh width, so the cut and the "…" reservation are
/// redone.
///
/// `one_face` is the easy case. `two_faces` alternates a body style with a
/// heavier heading, the order record traversal produces, which a single-slot
/// memo misses on every call.
///
/// The label is long enough that the cut keeps a short prefix: the whole
/// string is shaped once into the cached unbounded probe and only the prefix
/// is reshaped per width.
fn bench_ellipsis_churn(c: &mut Criterion, run: Run<'_>) {
    const HEADING_PX: f32 = 20.0;
    let shaper = TextShaper::new();
    let mut group = run.subgroup(c, "ellipsis_width_churn");

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
            let measured =
                measure_truncated(&mut text, slot, TEXT, width, BODY_PX, FontWeight::REGULAR);
            frame_end(&mut text, &shaper);
            black_box(measured.extent.size)
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
            // Body then heading, as a row records: one memo slot evicts the other's face.
            let body = measure_truncated(
                &mut text,
                slots[0],
                TEXT,
                width,
                BODY_PX,
                FontWeight::REGULAR,
            );
            let head = measure_truncated(
                &mut text,
                slots[1],
                TEXT,
                width,
                HEADING_PX,
                FontWeight::BOLD,
            );
            frame_end(&mut text, &shaper);
            black_box((body.extent.size, head.extent.size))
        });
    });
    group.finish();
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    bench_reuse_layer(c, run);
    bench_resize_drag(c, run);
    bench_ellipsis_churn(c, run);
}
