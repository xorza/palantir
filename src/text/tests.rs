use crate::common::hash::hash_str;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::record_store::RecordStore;
use crate::text::cosmic::{self, ClusterGlyph, CosmicMeasure};
use crate::text::internals::{TestMeasure, TestShape};
use crate::text::key::{ShapedTextRef, TextShapeKey};
use crate::text::mono;
use crate::text::probe;
use crate::text::system::{TextRunSlot, TextSystem};
use crate::text::wrap::{LineFit, TextWrap};
use crate::text::*;
use crate::widgets::theme::text_style::LINE_HEIGHT_MULT;
use rustc_hash::FxHashSet;

/// Measurement parameters with the defaults nearly every case wants:
/// bundled Inter Regular, unbounded, `HAlign::Auto`, and leading equal to
/// the font size — which keeps the mono fallback's line height numerically
/// equal to `font_size`, the placeholder layout the mono cases pin.
///
/// Override with struct-update syntax: `TestShape { max_width_px:
/// Some(32.0), ..shape(16.0) }`.
fn shape(font_size_px: f32) -> TestShape {
    TestShape {
        font_size_px,
        line_height_px: font_size_px,
        max_width_px: None,
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
        halign: HAlign::Auto,
    }
}

/// [`shape`] at production leading ([`LINE_HEIGHT_MULT`]) — what the real
/// UI shapes at, and what the cosmic geometry cases pin.
fn ui_shape(font_size_px: f32) -> TestShape {
    TestShape {
        line_height_px: font_size_px * LINE_HEIGHT_MULT,
        ..shape(font_size_px)
    }
}

fn slot(widget_id: WidgetId) -> TextRunSlot {
    slot_at(widget_id, 0)
}

fn slot_at(widget_id: WidgetId, ordinal: u16) -> TextRunSlot {
    TextRunSlot { widget_id, ordinal }
}

fn mono_shape(
    text: &str,
    font_size_px: f32,
    line_height_px: f32,
    max_width_px: Option<f32>,
    fit: LineFit,
) -> TestMeasure {
    let request = TextShapeRequest::unbounded(
        text,
        font_size_px,
        line_height_px,
        FontFamily::Sans,
        FontWeight::Regular,
    );
    let request = match max_width_px {
        Some(width) => request.bounded(width, HAlign::Auto, fit),
        None => request,
    };
    // Mono mints no shaped buffer, so every run it measures is invalid.
    let root = mono::internals::measure(request);
    TestMeasure {
        size: root.size,
        key: TextShapeKey::INVALID,
        intrinsic_min: root.intrinsic_min,
        single_line: root.single_line,
    }
}

fn measure_truncated(
    cosmic: &mut CosmicMeasure,
    text: &str,
    params: TestShape,
    fit: LineFit,
) -> TestMeasure {
    let unbounded = cosmic.measure(
        text,
        TestShape {
            max_width_px: None,
            halign: HAlign::Auto,
            ..params
        },
    );
    cosmic.measure_with_fit(text, params, fit, unbounded.key)
}

#[derive(Clone, Debug, PartialEq)]
struct GlyphPosition {
    x: f32,
    width: f32,
    line_top: f32,
    line_height: f32,
    start: usize,
    end: usize,
}

/// Glyph geometry in the same block-local space the renderer and probe
/// see — `left` off the buffer's own x, exactly as `extract_glyphs` folds
/// it into the run origin.
fn glyph_positions(cosmic: &CosmicMeasure, key: TextShapeKey) -> Vec<GlyphPosition> {
    let shaped = cosmic.shaped_run(key).expect("shaped buffer must exist");
    let left = shaped.left;
    shaped
        .buffer
        .layout_runs()
        .flat_map(move |run| {
            run.glyphs.iter().map(move |glyph| GlyphPosition {
                x: glyph.x - left,
                width: glyph.w,
                line_top: run.line_top,
                line_height: run.line_height,
                start: glyph.start,
                end: glyph.end,
            })
        })
        .collect()
}

#[test]
fn mono_measure_cases() {
    type Case = (&'static str, &'static str, f32, f32, Option<f32>, Size);
    let cases: &[Case] = &[
        ("empty", "", 16.0, 16.0, None, Size::ZERO),
        (
            "unbroken_legacy_short",
            "Hi",
            16.0,
            16.0,
            None,
            Size::new(16.0, 16.0),
        ),
        (
            "unbroken_legacy_long",
            "hello!!",
            16.0,
            16.0,
            None,
            Size::new(56.0, 16.0),
        ),
        (
            "wraps_below_unbroken",
            "12345678",
            16.0,
            16.0,
            Some(32.0),
            Size::new(32.0, 32.0),
        ),
        (
            "line_height_param_short",
            "Hi",
            16.0,
            24.0,
            None,
            Size::new(16.0, 24.0),
        ),
        (
            "line_height_param_wrapped",
            "12345678",
            16.0,
            24.0,
            Some(32.0),
            Size::new(32.0, 48.0),
        ),
    ];
    for (label, text, fs, lh_v, max_w, expected) in cases {
        let r = mono_shape(text, *fs, *lh_v, *max_w, LineFit::Wrap);
        assert_eq!(r.size, *expected, "case: {label}");
    }
    // Empty also produces the INVALID sentinel.
    assert!(
        mono_shape("", 16.0, 16.0, None, LineFit::Wrap)
            .key
            .is_invalid()
    );
}

/// `cursor_xy(...).x`. Mono fallback: each ASCII byte is
/// `font_size * 0.5` wide. Caret x is independent of `line_height`
/// (advance only depends on font_size + glyph). Empty string and
/// zero offset short-circuit to zero.
#[test]
fn cursor_xy_x_cases() {
    let cases: &[(&str, &str, usize, f32, f32, f32)] = &[
        ("zero_offset", "hello", 0, 16.0, 16.0, 0.0),
        ("empty_string", "", 0, 16.0, 16.0, 0.0),
        ("mono_one_char", "abc", 1, 16.0, 16.0, 8.0),
        ("mono_two_chars", "abc", 2, 16.0, 16.0, 16.0),
        ("mono_three_chars", "abc", 3, 16.0, 16.0, 24.0),
        ("lh_independent_short", "abc", 2, 16.0, 16.0, 16.0),
        ("lh_independent_tall", "abc", 2, 16.0, 24.0, 16.0),
    ];
    for (label, text, offset, fs, lh_v, expected) in cases {
        let m = TextShaper::test_mono();
        assert_eq!(
            m.cursor_xy(
                text,
                *offset,
                TestShape {
                    line_height_px: *lh_v,
                    ..shape(*fs)
                }
            )
            .x,
            *expected,
            "case: {label}"
        );
    }
}

#[test]
fn cache_key_discriminates_every_shaping_axis() {
    // The renderer caches shaped buffers by key, so any input that changes
    // glyph positions has to change the key. Miss one and a buffer shaped
    // for other parameters gets replayed — measured rect against the wrong
    // rasterized glyphs.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let base = c.measure("hi", shape(16.0)).key;

    for (label, variant, field, base_field) in [
        (
            "font size",
            shape(20.0),
            (|k: TextShapeKey| k.size_q) as fn(TextShapeKey) -> u32,
            base.size_q,
        ),
        (
            "line height",
            TestShape {
                line_height_px: 24.0,
                ..shape(16.0)
            },
            (|k: TextShapeKey| k.lh_q) as fn(TextShapeKey) -> u32,
            base.lh_q,
        ),
        (
            "family",
            TestShape {
                family: FontFamily::Mono,
                ..shape(16.0)
            },
            (|k: TextShapeKey| k.family_q as u32) as fn(TextShapeKey) -> u32,
            base.family_q as u32,
        ),
        (
            "weight",
            TestShape {
                weight: FontWeight::Bold,
                ..shape(16.0)
            },
            (|k: TextShapeKey| k.weight_q as u32) as fn(TextShapeKey) -> u32,
            base.weight_q as u32,
        ),
    ] {
        let key = c.measure("hi", variant).key;
        assert_ne!(base, key, "{label} must enter the cache key");
        assert_ne!(
            field(key),
            base_field,
            "{label} is the discriminating field"
        );
    }

    // The enum discriminants themselves are what land in the key, so a
    // variant reorder can't silently remap cached buffers.
    assert_eq!(base.family_q, FontFamily::Sans as u8);
    assert_eq!(base.weight_q, FontWeight::Regular as u8);
    assert_eq!(
        base.text_hash,
        hash_str("hi"),
        "direct shaping and authoring use the same canonical text hash",
    );
    assert_eq!(
        c.measure("hi", shape(16.0)).key,
        base,
        "the same request must be deterministic",
    );
}

#[test]
fn bundled_faces_resolve_and_their_metrics_differ() {
    // Each `FontFamily` / `FontWeight` pair must reach its intended
    // physical face. Asserted on the resolved family name and on advances,
    // not on the cache key — a key can discriminate perfectly while every
    // request silently falls back to one face.
    let mut c = CosmicMeasure::with_bundled_fonts();

    assert_eq!(
        c.resolved_family("M", FontFamily::Sans).as_deref(),
        Some("Inter"),
        "Sans must shape with the bundled Inter face",
    );
    assert_eq!(
        c.resolved_family("M", FontFamily::Mono).as_deref(),
        Some("JetBrains Mono"),
        "Mono must shape with the bundled JetBrains Mono face",
    );

    let width = |c: &mut CosmicMeasure, family, weight| {
        c.measure(
            "MMMM",
            TestShape {
                family,
                weight,
                ..shape(16.0)
            },
        )
        .size
        .w
    };
    let sans = width(&mut c, FontFamily::Sans, FontWeight::Regular);
    let sans_bold = width(&mut c, FontFamily::Sans, FontWeight::Bold);
    let mono = width(&mut c, FontFamily::Mono, FontWeight::Regular);
    let mono_bold = width(&mut c, FontFamily::Mono, FontWeight::Bold);

    assert!(sans > 0.0 && sans.is_finite());
    assert_ne!(
        sans, mono,
        "Inter (proportional) and JBMono (monospace) differ for 'MMMM'",
    );
    assert!(
        sans_bold > sans,
        "Inter Bold ({sans_bold}) must be wider than Regular ({sans}) — a \
         smaller-or-equal width means Bold silently fell back to Regular",
    );
    // The variable mono face instantiates `wght` without changing the cell
    // width, so weight-invariance here is the correct expectation.
    assert_eq!(
        mono, mono_bold,
        "monospace advance must be weight-invariant",
    );
}

#[test]
fn identity_cache_is_keyed_by_actual_shaping_inputs() {
    let mut text = TextSystem::mono();
    let wid = WidgetId::from_hash("a");
    let run_slot = slot(wid);
    let compact = shape(16.0);
    let r1 = text.shape_run(run_slot, "hi", compact, TextWrap::SingleLine);
    let calls = text.shaper.measure_calls();
    assert_eq!(r1.size, Size::new(16.0, 16.0));

    let same = text.shape_run(run_slot, "hi", compact, TextWrap::SingleLine);
    assert_eq!(same.size, r1.size);
    assert_eq!(same.key, r1.key);
    assert_eq!(same.intrinsic_min, r1.intrinsic_min);
    assert_eq!(
        text.shaper.measure_calls(),
        calls,
        "identical shaping inputs must reuse the row",
    );

    let quantized_same = text.shape_run(
        run_slot,
        "hi",
        TestShape {
            font_size_px: 16.006,
            line_height_px: 16.006,
            ..compact
        },
        TextWrap::SingleLine,
    );
    assert_eq!(quantized_same.key, same.key);
    assert_eq!(quantized_same.size, same.size);
    assert_eq!(quantized_same.intrinsic_min, same.intrinsic_min);
    assert_eq!(
        text.shaper.measure_calls(),
        calls,
        "raw values in the same 1/64 px bucket must reuse the canonical row",
    );

    let r2 = text.shape_run(
        run_slot,
        "hi",
        TestShape {
            line_height_px: 24.0,
            ..compact
        },
        TextWrap::SingleLine,
    );
    assert_eq!(r2.size, Size::new(16.0, 24.0));
    assert_eq!(
        text.shaper.measure_calls(),
        calls + 1,
        "metric changes must refresh the row",
    );

    let different_text = text.shape_run(run_slot, "hello", compact, TextWrap::SingleLine);
    assert_eq!(different_text.size, Size::new(40.0, 16.0));
    assert_eq!(
        text.shaper.measure_calls(),
        calls + 2,
        "text changes must refresh the row",
    );
}

#[test]
fn identity_cache_refreshes_stale_unbounded_and_bounded_results() {
    let mut text = TextSystem::mono();
    let wid = WidgetId::from_hash("a");
    let params = shape(16.0);

    let old = text.shape_run(slot(wid), "hi", params, TextWrap::SingleLine);
    assert_eq!(old.size, Size::new(16.0, 16.0));
    assert_eq!(
        text.shape_run(
            slot(wid),
            "hi",
            TestShape {
                max_width_px: Some(32.0),
                ..params
            },
            TextWrap::Wrap,
        )
        .size,
        Size::new(16.0, 16.0),
    );

    let current = text.shape_run(slot(wid), "abcdefgh", params, TextWrap::SingleLine);
    assert_eq!(current.size, Size::new(64.0, 16.0));
    // Eight 8 px glyphs at 32 px fit four per line: 32 px × two 16 px lines.
    assert_eq!(
        text.shape_run(
            slot(wid),
            "abcdefgh",
            TestShape {
                max_width_px: Some(32.0),
                ..params
            },
            TextWrap::Wrap,
        )
        .size,
        Size::new(32.0, 32.0),
    );
}

#[test]
fn text_wrap_policy_resolves_shape_and_layout_sizes_together() {
    #[derive(Clone, Copy, Debug)]
    struct Case {
        wrap: TextWrap,
        measured: Size,
        content: Size,
        min_content: Size,
        max_content: Size,
    }

    let mut text = TextSystem::mono();
    let widget_id = WidgetId::from_hash("wrap policy");
    let cases = [
        Case {
            wrap: TextWrap::SingleLine,
            measured: Size::new(56.0, 16.0),
            content: Size::new(56.0, 16.0),
            min_content: Size::new(56.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
        Case {
            wrap: TextWrap::Scroll,
            measured: Size::new(56.0, 16.0),
            content: Size::new(0.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(0.0, 16.0),
        },
        Case {
            wrap: TextWrap::Truncate,
            measured: Size::new(24.0, 16.0),
            content: Size::new(24.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
        Case {
            wrap: TextWrap::Ellipsis,
            measured: Size::new(24.0, 16.0),
            content: Size::new(24.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
        Case {
            wrap: TextWrap::Wrap,
            measured: Size::new(24.0, 48.0),
            content: Size::new(24.0, 48.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
        Case {
            wrap: TextWrap::WrapWithOverflow,
            measured: Size::new(32.0, 32.0),
            content: Size::new(32.0, 32.0),
            min_content: Size::new(32.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
    ];

    for (ordinal, case) in cases.into_iter().enumerate() {
        let request = TextShapeRequest::unbounded(
            "aa bbbb",
            16.0,
            16.0,
            FontFamily::Sans,
            FontWeight::Regular,
        );
        let slot = slot_at(widget_id, ordinal as u16);
        let unbounded = text.root(slot, request);
        let resolved = text.measure(slot, request, case.wrap, HAlign::Auto, Some(24.0));
        assert_eq!(resolved.measured, case.measured, "{case:?}");
        assert_eq!(
            case.wrap.content_size(resolved.measured),
            case.content,
            "{case:?}"
        );
        assert_eq!(
            case.wrap.min_content(&unbounded),
            case.min_content,
            "{case:?}"
        );
        assert_eq!(
            case.wrap.max_content(&unbounded),
            case.max_content,
            "{case:?}"
        );
    }

    let empty = text.measure(
        slot_at(widget_id, cases.len() as u16),
        TextShapeRequest::unbounded("", 16.0, 16.0, FontFamily::Sans, FontWeight::Regular),
        TextWrap::Ellipsis,
        HAlign::Auto,
        Some(24.0),
    );
    assert_eq!(empty.measured, Size::ZERO);
    assert_eq!(TextWrap::Ellipsis.content_size(empty.measured), Size::ZERO);
    let empty_root = text.root(
        slot_at(widget_id, cases.len() as u16),
        TextShapeRequest::unbounded("", 16.0, 16.0, FontFamily::Sans, FontWeight::Regular),
    );
    assert_eq!(TextWrap::Ellipsis.min_content(&empty_root), Size::ZERO);
    assert_eq!(TextWrap::Ellipsis.max_content(&empty_root), Size::ZERO);
}

#[test]
fn text_shape_key_validity_is_tagged_by_text_hash() {
    assert!(TextShapeKey::INVALID.is_invalid());
    let real = TextShapeKey {
        text_hash: 1,
        ..TextShapeKey::INVALID
    };
    assert!(!real.is_invalid());
    let zero_hash = TextShapeKey {
        fit_q: LineFit::Ellipsis as u8,
        ..TextShapeKey::INVALID
    };
    assert!(zero_hash.is_invalid());
}

#[test]
fn cursor_xy_walks_with_the_paragraph_direction() {
    // Caret-x at each byte boundary advances *with* the run's reading
    // direction and stays inside the line. Exact pixel values depend on
    // font metrics, so pin the invariants consumers rely on: monotonicity
    // along the reading direction, and both endpoints on the correct edge.
    let shaper = TextShaper::new();
    let shape = ui_shape(16.0);
    let carets = |text: &str| -> Vec<f32> {
        (0..=text.len())
            .map(|i| shaper.cursor_xy(text, i, shape).x)
            .collect()
    };

    let ltr = carets("hello");
    assert_eq!(ltr[0], 0.0, "an LTR line starts its caret at the left edge");
    for w in ltr.windows(2) {
        assert!(
            w[1] >= w[0] - 0.01,
            "LTR caret-x must be non-decreasing, got {ltr:?}",
        );
    }
    assert!(
        *ltr.last().unwrap() > ltr[0],
        "a non-empty LTR run ends right of where it began",
    );

    // Hebrew shapes right-to-left, so byte 0 sits at the *right* edge and
    // the caret walks leftwards. A glyph-start scan would report each
    // glyph's left edge instead, putting the caret a full character off
    // and making the sequence oscillate rather than descend.
    let rtl_text = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}";
    let rtl = carets(rtl_text);
    let line_right = shaper.measure(rtl_text, shape).size.w;
    // The measured extent has to span every glyph rather than stop at the
    // last one in the array, which in an RTL run is the leftmost — that
    // would report a four-character run as one character wide and let the
    // backend clip it.
    let one_char = shaper.measure("\u{5e9}", shape).size.w;
    assert!(
        line_right > one_char * 2.0,
        "an RTL run must measure its full extent: {line_right} vs one glyph {one_char}",
    );
    for w in rtl.windows(2) {
        assert!(
            w[1] <= w[0] + 0.01,
            "RTL caret-x must be non-increasing, got {rtl:?}",
        );
    }
    assert!(
        (rtl[0] - line_right).abs() < 1.0,
        "an RTL line starts its caret at the right edge: {} vs width {line_right}",
        rtl[0],
    );
    // The leftmost glyph's x is a sum of f32 advances from real font metrics,
    // so the end caret lands within rounding of 0, not exactly on it.
    assert!(
        rtl.last().unwrap().abs() < 0.01,
        "an RTL line ends its caret at the left edge, got {}",
        rtl.last().unwrap(),
    );
    // Direction is what separates the two, not merely text content.
    assert_ne!(ltr[0], rtl[0]);
}

#[test]
fn byte_at_xy_mono_fallback() {
    // Mono shaper: glyph_w = font_size * 0.5 = 8 px at 16 px font.
    // `byte_at_xy` ignores y on the mono path. Picks the boundary
    // whose prefix-x is closest to `target_x`.
    let m = TextShaper::test_mono();
    let cases: &[(&str, f32, usize)] = &[
        ("origin", 0.0, 0),
        ("first_boundary", 8.0, 1),
        ("mid_glyph_rounds_to_nearer_boundary", 11.0, 1),
        ("mid_glyph_rounds_to_nearer_boundary_other", 13.0, 2),
        ("past_end_clamps", 100.0, 5),
    ];
    for (label, x, expected) in cases {
        let got = m.byte_at_xy("hello", *x, 0.0, shape(16.0));
        assert_eq!(got, *expected, "case: {label}");
    }
}

#[test]
fn byte_at_xy_cosmic_path_monotonic_and_bounded() {
    // Real shaping: caret at the cursor_xy of byte i must hit-test
    // back to a byte close to i; widths sweep monotonically.
    let m = TextShaper::new();
    let s = "hello";
    let fs = 16.0;
    let probes: Vec<usize> = (0..=s.len())
        .map(|i| {
            let x = m.cursor_xy(s, i, ui_shape(fs)).x;
            m.byte_at_xy(s, x, 0.0, ui_shape(fs))
        })
        .collect();
    // Monotone non-decreasing — hit-test never goes backwards as x grows.
    for w in probes.windows(2) {
        assert!(w[1] >= w[0], "byte_at_xy not monotone: {probes:?}");
    }
    // Past-end x clamps to text.len().
    let past = m.byte_at_xy(s, 10_000.0, 0.0, ui_shape(fs));
    assert_eq!(past, s.len(), "x past end must clamp to text.len()");
}

/// An empty range emits nothing — and, being a sink rather than a
/// container, leaves whatever the caller already had alone. Clearing
/// belongs to the caller now (`resolve_geometry` does it unconditionally
/// before probing), because the only caller that retains rects owns a
/// reused buffer and a sink that cleared it would be reaching past its
/// job.
#[test]
fn selection_rects_empty_range_emits_nothing_and_touches_no_buffer() {
    let m = TextShaper::new();
    let mut out: Vec<Rect> = Vec::new();
    let pre = Rect::new(1.0, 2.0, 3.0, 4.0);
    out.push(pre); // pre-populate
    m.probe_layout("hello", ui_shape(16.0), |layout| {
        assert_eq!(layout.request.key.text_hash, hash_str("hello"));
        layout.selection_rects(5..5, &mut |rect| out.push(rect));
    });
    assert_eq!(
        out.as_slice(),
        [pre],
        "empty range emits nothing and leaves the caller's buffer untouched",
    );
}

#[test]
fn selection_rects_match_cosmic_highlight_spans() {
    #[derive(Debug)]
    struct Case {
        label: &'static str,
        text: &'static str,
        range: std::ops::Range<usize>,
        max_width_px: Option<f32>,
    }

    let m = TextShaper::new();
    let cases = [
        Case {
            label: "single_line",
            text: "hello",
            range: 1..4,
            max_width_px: None,
        },
        Case {
            label: "hard_breaks",
            text: "abc\ndef\nghi",
            range: 0..11,
            max_width_px: None,
        },
        // "def" only — lines before AND after the range must emit nothing.
        Case {
            label: "middle_line_only",
            text: "abc\ndef\nghi",
            range: 4..7,
            max_width_px: None,
        },
        // "ef\ng" — spans lines 1–2, line 0 must emit nothing.
        Case {
            label: "tail_span",
            text: "abc\ndef\nghi",
            range: 5..9,
            max_width_px: None,
        },
        Case {
            label: "mixed_bidi",
            text: "abc אבג def",
            range: 2..12,
            max_width_px: None,
        },
        Case {
            label: "soft_wrap_and_graphemes",
            text: "á one two three four five",
            range: 0..27,
            max_width_px: Some(48.0),
        },
    ];
    for case in cases {
        let params = TestShape {
            max_width_px: case.max_width_px,
            ..ui_shape(16.0)
        };
        let mut expected = Vec::new();
        m.probe_layout(case.text, params, |layout| {
            let buffer = layout.buffer_for_test().unwrap();
            let start = probe::cursor_from_byte(case.text, case.range.start);
            let end = probe::cursor_from_byte(case.text, case.range.end);
            for run in buffer.layout_runs() {
                // Raw `highlight` marks any run whose line differs from both
                // cursors as fully selected; cosmic's editor guards it with
                // this line-range check, so the oracle must too.
                if run.line_i < start.line || run.line_i > end.line {
                    continue;
                }
                expected.extend(
                    run.highlight(start, end)
                        .map(|(x, w)| Rect::new(x, run.line_top, w, run.line_height)),
                );
            }
        });

        let mut actual: Vec<Rect> = Vec::new();
        m.probe_layout(case.text, params, |layout| {
            layout.selection_rects(case.range, &mut |rect| actual.push(rect));
        });
        assert_eq!(
            actual.as_slice(),
            expected.as_slice(),
            "case: {}",
            case.label
        );
        // Independent of the oracle (which shares the line-range guard):
        // hand-computed placement for the partial-range cases — the three
        // unwrapped source lines sit at y = 0, lh, 2·lh.
        let lh = 16.0 * LINE_HEIGHT_MULT;
        let ys: Vec<f32> = actual.iter().map(|r| r.min.y).collect();
        match case.label {
            "single_line" => {
                assert_eq!(ys.len(), 1, "single-line range → one rect, got {ys:?}");
                assert!(actual[0].size.w > 0.0, "rect has positive width");
                assert!(actual[0].size.h > 0.0, "rect has positive height");
                assert!(ys[0].abs() < 0.5, "the only line sits at y≈0, got {ys:?}");
            }
            "middle_line_only" => {
                assert_eq!(ys.len(), 1, "one rect for the middle line, got {ys:?}");
                assert!((ys[0] - lh).abs() < 0.5, "rect sits on line 1, got {ys:?}");
            }
            "tail_span" => {
                assert_eq!(ys.len(), 2, "one rect per selected line, got {ys:?}");
                assert!((ys[0] - lh).abs() < 0.5, "first rect on line 1, got {ys:?}");
                assert!(
                    (ys[1] - 2.0 * lh).abs() < 0.5,
                    "second rect on line 2, got {ys:?}"
                );
            }
            _ => {}
        }
    }
}

#[test]
fn cursor_byte_round_trip_multiline() {
    // `cursor_from_byte` and `cursor_to_byte` must invert each other
    // across line breaks. Offsets sampled at every byte position of a
    // 3-line string with varying line lengths.
    let text = "ab\ncde\nfg";
    for off in 0..=text.len() {
        let cur = probe::cursor_from_byte(text, off);
        let back = probe::cursor_to_byte(text, cur);
        assert_eq!(
            back, off,
            "round-trip failed at offset {off}, cursor={cur:?}"
        );
    }
    // Line counts: offsets 0..=2 → line 0; 3..=6 → line 1; 7..=9 → line 2.
    assert_eq!(probe::cursor_from_byte(text, 0).line, 0);
    assert_eq!(probe::cursor_from_byte(text, 2).line, 0);
    assert_eq!(probe::cursor_from_byte(text, 3).line, 1);
    assert_eq!(probe::cursor_from_byte(text, 6).line, 1);
    assert_eq!(probe::cursor_from_byte(text, 7).line, 2);
    assert_eq!(probe::cursor_from_byte(text, 9).line, 2);
}

#[test]
fn cursor_xy_multiline_y_top_advances_per_line() {
    // Two-line buffer: caret on line 1 must have y_top > caret on line 0,
    // and the delta must be ≈ line_height. Pins multi-line caret routing
    // through cosmic's layout_runs.
    let m = TextShaper::new();
    let fs = 16.0;
    let lh_v = fs * LINE_HEIGHT_MULT;
    let p0 = m.cursor_xy("abc\ndef", 0, ui_shape(fs));
    let p1 = m.cursor_xy("abc\ndef", 4, ui_shape(fs));
    assert!(p0.y_top.abs() < 0.5, "line 0 y_top ≈ 0, got {}", p0.y_top);
    assert!(
        (p1.y_top - lh_v).abs() < 2.0,
        "line 1 y_top ≈ line_height ({lh_v}), got {}",
        p1.y_top,
    );
}

#[test]
fn cosmic_empty_text_returns_invalid_zero_size() {
    // Empty-text early-return on the cosmic path: ZERO size, INVALID
    // key, zero intrinsic_min. Pins the renderer's "drop INVALID
    // runs" contract for empty strings.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let r = c.measure("", ui_shape(16.0));
    assert_eq!(r.size, Size::ZERO);
    assert!(r.key.is_invalid());
    assert_eq!(r.intrinsic_min, 0.0);
    // `buffer_for(INVALID)` must return None — even after measuring,
    // no buffer was cached for the empty input.
    assert!(c.shaped_run(r.key).is_none());

    let shaper = TextShaper::test_mono();
    let calls = shaper.measure_calls();
    let r = shaper.measure("", ui_shape(16.0));
    assert_eq!(r.size, Size::ZERO);
    assert_eq!(r.intrinsic_min, 0.0);
    assert_eq!(shaper.measure_calls(), calls);
}

#[test]
fn invalid_metrics_panic_before_any_shaping_dispatch() {
    use crate::primitives::approx::EPS;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let cases = [
        ("zero font", 0.0, 16.0),
        ("negative font", -1.0, 16.0),
        ("sub-epsilon font", EPS * 0.5, 16.0),
        ("epsilon font", EPS, 16.0),
        ("NaN font", f32::NAN, 16.0),
        ("infinite font", f32::INFINITY, 16.0),
        ("zero line height", 16.0, 0.0),
        ("negative line height", 16.0, -1.0),
        ("sub-epsilon line height", 16.0, EPS * 0.5),
        ("epsilon line height", 16.0, EPS),
        ("NaN line height", 16.0, f32::NAN),
        ("infinite line height", 16.0, f32::INFINITY),
    ];
    let mono = TextShaper::test_mono();
    let cosmic = TextShaper::new();
    for (label, font_size_px, line_height_px) in cases {
        let params = TestShape {
            font_size_px,
            line_height_px,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        };
        for shaper in [&mono, &cosmic] {
            let calls = shaper.measure_calls();
            assert!(
                catch_unwind(AssertUnwindSafe(|| shaper.measure("hi", params))).is_err(),
                "{label}: invalid metrics must panic at request construction",
            );
            assert_eq!(
                shaper.measure_calls(),
                calls,
                "{label}: invalid metrics reached a shaping dispatch",
            );
        }
    }
}

#[test]
fn identity_cache_rejects_invalid_metrics_before_dispatch() {
    use crate::primitives::approx::EPS;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let widget_id = WidgetId::from_hash("invalid metrics");
    let calls = shaper.measure_calls();

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        text.shape_run(
            slot(widget_id),
            "hi",
            TestShape {
                line_height_px: 16.0,
                max_width_px: Some(40.0),
                halign: HAlign::Center,
                ..shape(EPS * 0.5)
            },
            TextWrap::Ellipsis,
        )
    }))
    .is_err();
    assert!(
        panicked,
        "invalid metrics must panic at request construction"
    );
    assert!(
        !text.has_entry(widget_id, 0),
        "invalid metrics entered the reuse cache",
    );
    assert_eq!(
        shaper.measure_calls(),
        calls,
        "invalid metrics reached a shaping dispatch",
    );
}

#[test]
fn fitting_truncate_returns_the_unbounded_root_without_reshaping() {
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let wid = WidgetId::from_hash("fitting truncate");
    let fitting = TestShape {
        max_width_px: Some(200.0),
        halign: HAlign::Center,
        ..shape(16.0)
    };
    let unbounded_shape = TestShape {
        max_width_px: None,
        halign: HAlign::Auto,
        ..fitting
    };

    for (ordinal, wrap) in [(0u16, TextWrap::Truncate), (1, TextWrap::Ellipsis)] {
        let run_slot = slot_at(wid, ordinal);
        let fit = wrap.line_fit().unwrap();
        let natural = text.shape_run(run_slot, "ok", unbounded_shape, wrap);
        let calls = text.shaper.measure_calls();

        let fitted = text.shape_run(run_slot, "ok", fitting, wrap);
        assert_eq!(
            fitted.key, natural.key,
            "a fitting {wrap:?} must reuse the unbounded root's identity",
        );
        assert_eq!(fitted.size, natural.size);
        assert_eq!(fitted.intrinsic_min, natural.intrinsic_min);
        assert_eq!(
            text.shaper.measure_calls(),
            calls,
            "a fitting {wrap:?} must not dispatch a second shape",
        );
        let bounded_key = fitting.request("ok", fit).key;
        assert!(
            !text.shaper.has_cosmic_buffer(bounded_key),
            "a fitting {wrap:?} must not mint a bounded cache entry",
        );

        // Over-wide text still resolves through the truncating path.
        let narrow = TestShape {
            max_width_px: Some(20.0),
            ..fitting
        };
        let truncated = text.shape_run(run_slot, "wider than twenty", narrow, wrap);
        assert_ne!(truncated.key, truncated.key.unbounded_version());
        assert_eq!(truncated.key.fit_q, fit as u8);
        assert!(truncated.size.w <= 20.0);
    }

    // A multi-line source collapses to its first line under Clip/Ellipsis,
    // so the unbounded root cannot stand in even when its widest line fits.
    let multiline = text.shape_run(slot_at(wid, 2), "a\nb", fitting, TextWrap::Ellipsis);
    let bounded_key = fitting.request("a\nb", LineFit::Ellipsis).key;
    assert_eq!(
        multiline.key, bounded_key,
        "multi-line text must resolve through the truncating path",
    );
    // One line at the 16 px leading `fitting` carries — the collapse is
    // what makes a two-line source measure a single line tall.
    assert_eq!(multiline.size.h, 16.0);
}

#[test]
fn bounded_width_canonicalizes_and_rejects_non_finite_values() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let base = TestShape {
        line_height_px: 19.2,
        ..shape(16.0)
    };
    let shaper = TextShaper::new();
    let unbounded = shaper.measure("hi", base);
    assert!(
        unbounded.key.max_width_px().is_none(),
        "None is the unbounded form",
    );
    let zero = shaper.measure(
        "hi",
        TestShape {
            max_width_px: Some(0.0),
            ..base
        },
    );
    assert_eq!(
        zero.key.max_width_px(),
        Some(0.0),
        "zero is a valid bounded width",
    );
    // Negative widths (over-constrained layouts) clamp to the zero-width key.
    let negative = shaper.measure(
        "hi",
        TestShape {
            max_width_px: Some(-1.0),
            ..base
        },
    );
    assert_eq!(negative.key, zero.key);
    for (label, width) in [
        ("NaN", f32::NAN),
        ("positive infinity", f32::INFINITY),
        ("negative infinity", f32::NEG_INFINITY),
    ] {
        let params = TestShape {
            max_width_px: Some(width),
            ..base
        };
        let calls = shaper.measure_calls();
        assert!(
            catch_unwind(AssertUnwindSafe(|| shaper.measure("hi", params))).is_err(),
            "{label}: non-finite width must panic at request construction",
        );
        assert_eq!(shaper.measure_calls(), calls, "{label}");
    }
}

#[test]
fn above_epsilon_metrics_survive_cache_key_canonicalization() {
    use crate::primitives::approx::EPS;

    let mut cosmic = CosmicMeasure::with_bundled_fonts();
    let result = cosmic.measure("x", shape(EPS * 2.0));
    assert!(!result.key.is_invalid());
    assert_eq!(result.key.size_q, 1);
    assert_eq!(result.key.lh_q, 1);
    assert!(cosmic.shaped_run(result.key).is_some());
}

#[test]
fn cosmic_intrinsic_min_tracks_the_widest_unbreakable_segment() {
    // `intrinsic_min` is the wrap floor: the width of the widest segment
    // no line break can split. Break opportunities are UAX #14's — the
    // same ones cosmic-text splits its shape words on — so the floor has
    // to track punctuation and script boundaries, not just whitespace.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let shape = ui_shape(16.0);

    // (run, the widest segment its floor must land on)
    for (text, widest) in [
        // "world" outweighs "hello" in Inter — `w` is the wider glyph.
        ("hello world hi", "world"),
        // A hyphen opens a break after itself, so the floor is the
        // prefix *including* it, not the whole token.
        ("aaa-bbb", "aaa-"),
        // Trailing punctuation binds to the word it follows.
        ("aaa, bbb", "aaa,"),
        // No whitespace anywhere: every ideograph is its own segment, so
        // a CJK paragraph must floor at one glyph rather than one line.
        (
            "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30c6}\u{30ad}\u{30b9}\u{30c8}",
            "\u{65e5}",
        ),
    ] {
        let full = c.measure(text, shape);
        let segment = c.measure(widest, shape);
        assert!(
            full.intrinsic_min < full.size.w,
            "{text:?} must break somewhere: floor {} vs total width {}",
            full.intrinsic_min,
            full.size.w,
        );
        // Kerning across a break can shift the in-run segment width a
        // little against its standalone measurement, so allow ±15%.
        let rel_err = (full.intrinsic_min - segment.intrinsic_min).abs() / segment.intrinsic_min;
        assert!(
            rel_err < 0.15,
            "{text:?} floor ({}) must be the width of {widest:?} ({}), rel_err = {rel_err}",
            full.intrinsic_min,
            segment.intrinsic_min,
        );
    }

    // A no-break space opens no opportunity, so it neither splits its run
    // nor hangs — its own advance counts toward the segment.
    let nbsp = c.measure("aaa\u{a0}bbb", shape);
    assert!(
        (nbsp.intrinsic_min - nbsp.size.w).abs() < 2.0,
        "no-break space must keep one segment: floor {} vs width {}",
        nbsp.intrinsic_min,
        nbsp.size.w,
    );

    // Single-segment input: intrinsic_min ≈ size.w. size.w is the last
    // glyph's (x + w) ceil'd; intrinsic_min sums glyph widths. The two
    // differ by sub-pixel kerning / ceil rounding — allow 2 px.
    let hello = c.measure("hello", shape);
    assert!(
        (hello.intrinsic_min - hello.size.w).abs() < 2.0,
        "single word: intrinsic_min ({}) ≈ size.w ({})",
        hello.intrinsic_min,
        hello.size.w,
    );

    // Width-bounded shapes skip the segment scan and report a zero floor —
    // every consumer derives it from the unbounded root instead.
    let full = c.measure("hello world hi", shape);
    let bounded = c.measure(
        "hello world hi",
        TestShape {
            max_width_px: Some(60.0),
            ..shape
        },
    );
    assert!(bounded.size.h > full.size.h, "60 px must force a wrap");
    assert_eq!(
        bounded.intrinsic_min, 0.0,
        "bounded shapes must not pay the segment scan",
    );
}

#[test]
fn cache_key_collapses_halign_when_unbounded() {
    // Halign only moves glyphs when there is a wrap target to align
    // within, so the key folds it down to Auto without one — single-line
    // callers don't pay an N-way cache split. With a target it must
    // discriminate, or two alignments share one shaped buffer.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let key = |c: &mut CosmicMeasure, halign, max_width_px| {
        c.measure(
            "hi",
            TestShape {
                halign,
                max_width_px,
                ..shape(16.0)
            },
        )
        .key
    };
    let unbounded_auto = key(&mut c, HAlign::Auto, None);
    let bounded_auto = key(&mut c, HAlign::Auto, Some(200.0));
    for halign in [HAlign::Left, HAlign::Center, HAlign::Right] {
        assert_eq!(
            key(&mut c, halign, None),
            unbounded_auto,
            "unbounded: {halign:?} must collapse to Auto in the key",
        );
        assert_ne!(
            key(&mut c, halign, Some(200.0)),
            bounded_auto,
            "wrap-bounded: {halign:?} must enter the key",
        );
    }
}

#[test]
fn bounded_identity_cache_keys_width_and_halign() {
    let m = TextShaper::new();
    let mut text = TextSystem::new(m.clone());
    let wid = WidgetId::from_hash("w");
    let params = shape(16.0);
    text.shape_run(slot(wid), "hi", params, TextWrap::SingleLine);
    let baseline = m.measure_calls();

    text.shape_run(
        slot(wid),
        "hi",
        TestShape {
            max_width_px: Some(200.0),
            halign: HAlign::Left,
            ..params
        },
        TextWrap::Wrap,
    );
    let after_left = m.measure_calls();
    assert_eq!(after_left, baseline + 1, "first wrap shape must dispatch");

    text.shape_run(
        slot(wid),
        "hi",
        TestShape {
            max_width_px: Some(200.0),
            halign: HAlign::Left,
            ..params
        },
        TextWrap::Wrap,
    );
    assert_eq!(
        m.measure_calls(),
        after_left,
        "identical wrap call must hit cache"
    );

    text.shape_run(
        slot(wid),
        "hi",
        TestShape {
            max_width_px: Some(200.0),
            halign: HAlign::Right,
            ..params
        },
        TextWrap::Wrap,
    );
    assert_eq!(
        m.measure_calls(),
        after_left + 1,
        "halign change at same target must bust wrap reuse",
    );

    text.shape_run(
        slot(wid),
        "hi",
        TestShape {
            max_width_px: Some(201.0),
            halign: HAlign::Right,
            ..params
        },
        TextWrap::Wrap,
    );
    assert_eq!(
        m.measure_calls(),
        after_left + 2,
        "width change must bust wrap reuse",
    );
}

/// A reuse row lives exactly one frame of non-use: `end_frame` drops
/// every row whose hot bit wasn't set during the frame, with no size
/// threshold gating the pass. The old exponential ladder held cold rows
/// until the map crossed a power-of-two rung (256 at minimum), so a
/// three-row window swept nothing — the second phase here is what that
/// version could not do.
#[test]
fn end_frame_drops_every_row_not_used_this_frame() {
    let mut text = TextSystem::mono();
    let a = WidgetId::from_hash("a");
    let b = WidgetId::from_hash("b");
    let params = shape(16.0);

    text.shape_run(slot_at(a, 0), "hi", params, TextWrap::SingleLine);
    text.shape_run(slot_at(a, 1), "hi", params, TextWrap::SingleLine);
    text.shape_run(slot(b), "yo", params, TextWrap::SingleLine);
    text.end_frame(&FxHashSet::default());
    assert_eq!(text.entry_count(), 3, "rows used this frame all survive");

    // Second frame touches only `a`'s first row. Three rows is far below
    // the old ladder's floor, so all three used to survive.
    text.shape_run(slot_at(a, 0), "hi", params, TextWrap::SingleLine);
    text.end_frame(&FxHashSet::default());
    assert_eq!(text.entry_count(), 1);
    assert!(text.has_entry(a, 0), "row re-shaped this frame stays hot");
    assert!(!text.has_entry(a, 1), "untouched sibling row goes");
    assert!(!text.has_entry(b, 0), "untouched row of another widget too");

    // A removed widget's rows go even when hot, in the same retain pass
    // that drops cold ones.
    text.shape_run(slot_at(a, 0), "hi", params, TextWrap::SingleLine);
    text.shape_run(slot(b), "yo", params, TextWrap::SingleLine);
    text.end_frame(&FxHashSet::from_iter([a]));
    assert_eq!(text.entry_count(), 1);
    assert!(
        !text.has_entry(a, 0),
        "removed widget's row goes regardless of its hot bit",
    );
    assert!(text.has_entry(b, 0), "unrelated hot row remains");
}

/// Right-aligned multi-line buffer: caret at byte 4 ("abc\n|") lands
/// on the empty second line. Cosmic's per-line halign offset only
/// shifts existing glyphs, so an empty line has `line_w = 0` and
/// cosmic reports `x = 0` whatever the alignment; the empty-line branch
/// routes through `empty_line_x` to put the caret where the first typed
/// glyph will actually appear.
///
/// That edge is the *block's*, not the wrap target's: the block is what
/// the owner aligns inside its rect, so measuring the caret against the
/// wrap target here would align it a second time and carry it past the
/// text it belongs to.
#[test]
fn cursor_xy_on_empty_line_respects_right_align() {
    let m = TextShaper::new();
    let text = "abc\n";
    let wrap = 200.0;
    let font = 16.0;
    // `cursor_xy` calls `with_buffer` which in turn drives
    // `measure` end-to-end (unbounded + wrap-shape), so no
    // pre-prime is needed — the shaper builds whatever cache
    // entry it needs on first hit.
    let shape = TestShape {
        max_width_px: Some(wrap),
        halign: HAlign::Right,
        ..ui_shape(font)
    };
    let block = m.measure(text, shape).size.w;
    let pos = m.cursor_xy(text, text.len(), shape);
    assert!(
        (pos.x - block).abs() < 0.5,
        "right-aligned caret on empty trailing line must sit at the \
         block's right edge ({block}); got x = {}",
        pos.x,
    );
    assert!(
        block < wrap - 100.0,
        "\"abc\" must be far narrower than the {wrap} px wrap target, \
         or this cannot tell the block edge from the wrap target; got {block}",
    );
    // And the left-aligned counterpart still anchors at zero —
    // sanity-pins the helper isn't accidentally always returning
    // the right edge.
    let pos_left = m.cursor_xy(
        text,
        text.len(),
        TestShape {
            max_width_px: Some(wrap),
            halign: HAlign::Left,
            ..ui_shape(font)
        },
    );
    assert!(
        pos_left.x.abs() < 0.5,
        "left-aligned caret on empty trailing line stays at 0; \
         got x = {}",
        pos_left.x,
    );
}

#[test]
fn cosmic_ellipsis_elides_long_line_to_width() {
    // A label wider than the committed width truncates to one line that
    // fits, with a trailing ellipsis. Pins the "labels never overflow
    // their box" contract the Button relies on.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "Screenshot 2026-05-28 at 01.21.25.png";
    let w = 120.0;
    let elided = measure_truncated(
        &mut c,
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
        LineFit::Ellipsis,
    );
    // Precondition: the natural single line genuinely overflows `w`.
    let full = c.measure(long, shape(16.0));
    assert!(
        full.size.w > w,
        "precondition: natural line ({}) must overflow the cap ({w})",
        full.size.w,
    );
    // Elided result fits the cap (ceil tolerance) and stays one line.
    assert!(
        elided.size.w <= w + 1.0,
        "elided width {} must fit cap {w}",
        elided.size.w,
    );
    assert!(
        elided.size.h <= (16.0 * LINE_HEIGHT_MULT).ceil() + 0.5,
        "elided run must be a single line, got h={}",
        elided.size.h,
    );
    assert_eq!(
        elided.intrinsic_min, 0.0,
        "an elided run has zero min floor"
    );
    let zero_width = measure_truncated(
        &mut c,
        long,
        TestShape {
            max_width_px: Some(0.0),
            ..shape(16.0)
        },
        LineFit::Ellipsis,
    );
    assert_eq!(
        zero_width.size.w, 0.0,
        "an ellipsis that cannot fit collapses to zero width",
    );
    // The elided buffer must not collide with the *wrapped* buffer at the
    // same width — they hold different strings, so distinct cache keys.
    let wrapped = c.measure(
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
    );
    assert_ne!(
        elided.key, wrapped.key,
        "elision and wrap must key distinct cache slots at the same width",
    );
}

#[test]
fn fitting_prefix_cuts_on_logical_cluster_boundaries() {
    // Hand-built glyph runs, so the cut is checked against arithmetic
    // rather than against whatever the installed fonts happen to measure.
    // Each entry is (start, end, advance) in *visual* order.
    type Run = &'static [(usize, usize, f32)];
    // "abc", 10 px per glyph, LTR: visual order is logical order.
    const LTR: Run = &[(0, 1, 10.0), (1, 2, 10.0), (2, 3, 10.0)];
    // The same three glyphs read right-to-left: the logically-first glyph
    // is emitted last. A cut driven by visual order would keep the wrong
    // end of the run.
    const RTL: Run = &[(2, 3, 10.0), (1, 2, 10.0), (0, 1, 10.0)];
    // "a🇺🇸": one cluster (bytes 1..9) shaping to two 10 px glyphs. Paying
    // for one of them must not commit the whole cluster's bytes.
    const CLUSTER: Run = &[(0, 1, 10.0), (1, 9, 10.0), (1, 9, 10.0)];
    // "á" decomposed: a zero-width mark glyph sharing the base's cluster
    // costs nothing, so it must not hold the cut back.
    const MARK: Run = &[(0, 3, 10.0), (0, 3, 0.0), (3, 4, 10.0)];

    const ANY: usize = usize::MAX;
    let mut order = Vec::new();
    for (run, avail, max_end, expected, why) in [
        (LTR, 0.0, ANY, 0, "no budget keeps nothing"),
        (LTR, 9.9, ANY, 0, "a glyph is all-or-nothing"),
        (LTR, 10.0, ANY, 1, "an exact fit is a fit"),
        (LTR, 25.0, ANY, 2, "the third glyph would overrun"),
        (LTR, 30.0, ANY, 3, "the whole run fits"),
        (LTR, 1000.0, ANY, 3, "surplus budget keeps the whole run"),
        (
            RTL,
            10.0,
            ANY,
            1,
            "RTL keeps the logical prefix, not the visual one",
        ),
        (RTL, 25.0, ANY, 2, "RTL cut tracks logical order"),
        (RTL, 30.0, ANY, 3, "the whole RTL run fits"),
        (
            CLUSTER,
            10.0,
            ANY,
            1,
            "one glyph of the cluster is unaffordable",
        ),
        (
            CLUSTER,
            25.0,
            ANY,
            1,
            "25 px pays for only one of the two cluster glyphs",
        ),
        (CLUSTER, 30.0, ANY, 9, "30 px pays for the whole cluster"),
        (
            MARK,
            10.0,
            ANY,
            3,
            "a zero-width mark rides along with its base",
        ),
        (MARK, 20.0, ANY, 4, "the following glyph is affordable too"),
        // `max_end` drives the back-off: feeding back the previous answer
        // must retire at least one more cluster, all the way to nothing.
        (LTR, 1000.0, 3, 2, "the bound retires the last glyph"),
        (LTR, 1000.0, 2, 1, "and the one before it"),
        (LTR, 1000.0, 1, 0, "and the last one standing"),
        (LTR, 1000.0, 0, 0, "an exhausted bound stays at nothing"),
        (RTL, 1000.0, 3, 2, "the bound reads logical order too"),
        (
            CLUSTER,
            1000.0,
            9,
            1,
            "backing off a cluster retires all of its glyphs",
        ),
    ] {
        let cut = cosmic::fitting_prefix(
            run.len(),
            |i| ClusterGlyph {
                start: run[i].0,
                end: run[i].1,
                advance: run[i].2,
            },
            &mut order,
            avail,
            max_end,
        );
        assert_eq!(cut, expected, "avail={avail} max_end={max_end}: {why}");
        // Every bounded cut falls strictly below its bound, so feeding the
        // previous answer back always makes progress — that is what makes
        // the back-off terminate. Zero is the floor it terminates *at*: the
        // production loop stops on an empty cut and never re-bounds by it.
        assert!(
            max_end == ANY || max_end == 0 || cut < max_end,
            "a bounded cut must fall strictly below its bound",
        );
    }
}

#[test]
fn ellipsis_never_measures_wider_than_its_budget() {
    // Two ways a cut overruns the budget it was measured against: paying
    // for only some of a cluster's glyphs while committing all of its bytes
    // (flag and ZWJ emoji), and reshaping a prefix whose last letter changes
    // form once it lands at a word end (Arabic medial → final). Both resolve
    // through fonts this crate does not bundle, so the bound — never an exact
    // width — is what holds on every machine.
    let base = shape(16.0);
    for text in [
        "flag \u{1f1fa}\u{1f1f8} emoji \u{1f600} run",
        "\u{1f469}\u{200d}\u{1f469}\u{200d}\u{1f467} family emoji",
        "\u{627}\u{644}\u{633}\u{644}\u{627}\u{645} \u{639}\u{644}\u{64a}\u{643}\u{645}",
        "\u{645}\u{631}\u{62d}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645}",
    ] {
        for family in [FontFamily::Sans, FontFamily::Mono] {
            for fit in [LineFit::Clip, LineFit::Ellipsis] {
                let mut c = CosmicMeasure::with_bundled_fonts();
                for width_px in 0..=160 {
                    let width = width_px as f32;
                    let m = measure_truncated(
                        &mut c,
                        text,
                        TestShape {
                            max_width_px: Some(width),
                            family,
                            ..base
                        },
                        fit,
                    );
                    // Widths are whole pixels and `size.w` is ceil'd, so a
                    // run that fits its budget cannot round past it.
                    assert!(
                        m.size.w <= width,
                        "{family:?} {fit:?} {text:?}: measured {} against budget {width}",
                        m.size.w,
                    );
                }
            }
        }
    }
}

#[test]
fn ellipsis_keeps_the_logical_prefix_in_both_reading_directions() {
    // The cut walks the cached unbounded shape's glyphs, which arrive in
    // *visual* order. In an RTL run the logically-first glyph sits at the
    // right edge and trailing edges descend, so a cut driven by `x + w`
    // stops at the first glyph and drops the whole run.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let unbounded = shape(16.0);
    let elide = |c: &mut CosmicMeasure, text: &str, width: f32| {
        measure_truncated(
            c,
            text,
            TestShape {
                max_width_px: Some(width),
                ..unbounded
            },
            LineFit::Ellipsis,
        )
        .size
        .w
    };

    let rtl = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}";
    let marker_only = c.measure("\u{2026}", unbounded).size.w;

    // Room for the marker plus one glyph: something of the run must survive.
    let one = elide(&mut c, rtl, 28.0);
    assert!(
        one > marker_only,
        "an RTL run with room to spare must keep text, got {one} vs bare marker {marker_only}",
    );

    // Room for two: the survivors must be the logical prefix. The logical
    // *suffix* is the wrong answer a visual-order cut reaches for, and
    // Hebrew glyph advances differ enough to tell the two apart.
    let two = elide(&mut c, rtl, 35.0);
    let prefix = c.measure("\u{5e9}\u{5dc}\u{2026}", unbounded).size.w;
    let suffix = c.measure("\u{5d5}\u{5dd}\u{2026}", unbounded).size.w;
    assert!(
        (two - prefix).abs() < 1.0,
        "RTL elision must keep the leading characters: {two} vs prefix {prefix}",
    );
    assert!(
        (two - suffix).abs() >= 1.0,
        "prefix and suffix widths must differ for this to prove anything: {two} vs {suffix}",
    );

    // LTR is the control: same code path, and widening the box must reveal
    // more of the run rather than less.
    let narrow = elide(&mut c, "abcd", 20.0);
    let wide = elide(&mut c, "abcd", 28.0);
    assert!(
        wide > narrow,
        "a wider box must keep more of an LTR run: {wide} vs {narrow}",
    );
}

#[test]
fn cosmic_ellipsis_short_text_not_truncated() {
    // A label that already fits the cap is shaped whole — no spurious
    // ellipsis, width matches the natural measurement.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let short = "ok";
    let natural = c.measure(short, shape(16.0));
    let elided = measure_truncated(
        &mut c,
        short,
        TestShape {
            max_width_px: Some(200.0),
            ..shape(16.0)
        },
        LineFit::Ellipsis,
    );
    assert!(
        (elided.size.w - natural.size.w).abs() <= 2.0,
        "short text must not truncate: elided {} vs natural {}",
        elided.size.w,
        natural.size.w,
    );
}

#[test]
fn cosmic_truncate_fits_measures_natural_width_regardless_of_halign() {
    // Regression: a single-line label that fits a wide cap must measure to
    // its natural glyph width, not inflate toward the box, even with a
    // non-`Auto` halign (the encoder positions the line; the shaped buffer
    // must not bake in width + per-line align). A `Center`-aligned label in
    // a 400 px cap previously measured ~half the box wide.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let label = "File";
    let cap = 400.0;
    let natural = c.measure(label, shape(16.0));
    for fit in [false, true] {
        let m = measure_truncated(
            &mut c,
            label,
            TestShape {
                max_width_px: Some(cap),
                halign: HAlign::Center,
                ..shape(16.0)
            },
            if fit {
                LineFit::Ellipsis
            } else {
                LineFit::Clip
            },
        );
        assert!(
            (m.size.w - natural.size.w).abs() <= 2.0,
            "centered fitting label must measure natural width ({}), got {} (with_ellipsis={fit})",
            natural.size.w,
            m.size.w,
        );
    }
}

#[test]
fn cosmic_singleline_clips_to_width_without_ellipsis() {
    // The default `SingleLine` mode (clip, no marker) cuts an over-wide
    // label to fit the cap on one line — like the ellipsis path but with no
    // trailing `…`, and reserving no room for one. Distinct cache slot from
    // both the wrapped and the ellipsized buffers at the same width.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "Screenshot 2026-05-28 at 01.21.25.png";
    let w = 120.0;
    let full = c.measure(long, shape(16.0));
    assert!(
        full.size.w > w,
        "precondition: natural line ({}) must overflow the cap ({w})",
        full.size.w,
    );
    let clipped = measure_truncated(
        &mut c,
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
        LineFit::Clip,
    );
    assert!(
        clipped.size.w <= w + 1.0,
        "clipped width {} must fit cap {w}",
        clipped.size.w,
    );
    assert!(
        clipped.size.h <= (16.0 * LINE_HEIGHT_MULT).ceil() + 0.5,
        "clipped run must be a single line, got h={}",
        clipped.size.h,
    );
    assert_eq!(
        clipped.intrinsic_min, 0.0,
        "a clipped run has zero min floor"
    );
    // Clip and ellipsis cut to the same cap but bake different strings (the
    // ellipsis path appends `…` and reserves its width), so they must key
    // distinct cache slots.
    let elided = measure_truncated(
        &mut c,
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
        LineFit::Ellipsis,
    );
    // Clip, ellipsis, and wrap each bake a distinct buffer at the same width.
    let wrapped = c.measure(
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
    );
    assert_ne!(
        clipped.key, elided.key,
        "clip and ellipsis must key distinctly"
    );
    assert_ne!(
        clipped.key, wrapped.key,
        "clip and wrap must key distinctly"
    );
    assert_eq!(
        clipped.key.text_hash, full.key.text_hash,
        "bounded keys reuse the source text hash",
    );
    assert_eq!(clipped.key.fit_q, LineFit::Clip as u8);
    assert_eq!(elided.key.fit_q, LineFit::Ellipsis as u8);
    assert_eq!(wrapped.key.fit_q, LineFit::Wrap as u8);
}

#[test]
fn mono_ellipsis_caps_width_with_zero_floor() {
    // Mono fallback: an elided long word caps at the available width and
    // reports zero min-content (shrinks to the ellipsis); the wrap
    // counterpart instead grows height and keeps the longest-word floor.
    let long = "abcdefghijklmnop"; // 16 ASCII bytes × 8 px = 128 px natural
    let w = 40.0;
    let elided = mono_shape(long, 16.0, 16.0, Some(w), LineFit::Ellipsis);
    assert_eq!(elided.size.w, w, "elided mono caps at the width");
    assert_eq!(elided.size.h, 16.0, "elided mono is one line");
    assert_eq!(elided.intrinsic_min, 0.0, "elided mono has zero floor");
    let wrapped = mono_shape(long, 16.0, 16.0, Some(w), LineFit::Wrap);
    assert!(wrapped.size.h > 16.0, "wrap grows height across lines");
    assert!(
        wrapped.intrinsic_min > 0.0,
        "wrap keeps a longest-word floor"
    );
}

/// Truncation reads its probe glyphs from the cached unbounded buffer.
/// Measure the same input on a fresh measurer and one containing unrelated
/// cached shapes; both the derived key and exact measurement must agree.
#[test]
fn truncation_from_cached_unbounded_is_order_independent() {
    let long = "the quick brown fox jumps over the lazy dog";
    let (fs, w) = (14.0, 80.0);

    // Fresh measurer: only the target measurement.
    let mut fresh = CosmicMeasure::with_bundled_fonts();
    let r_fresh = measure_truncated(
        &mut fresh,
        long,
        TestShape {
            max_width_px: Some(w),
            halign: HAlign::Left,
            ..shape(fs)
        },
        LineFit::Ellipsis,
    );

    // Reused measurer: populate unrelated unbounded, truncated, and ellipsis
    // cache entries first, then measure the identical target.
    let mut reused = CosmicMeasure::with_bundled_fonts();
    measure_truncated(
        &mut reused,
        "a considerably longer string that grows the probe buffer capacity",
        TestShape {
            max_width_px: Some(220.0),
            family: FontFamily::Mono,
            halign: HAlign::Left,
            ..shape(20.0)
        },
        LineFit::Ellipsis,
    );
    measure_truncated(
        &mut reused,
        "short",
        TestShape {
            max_width_px: Some(30.0),
            halign: HAlign::Left,
            ..shape(10.0)
        },
        LineFit::Clip,
    );
    let r_reused = measure_truncated(
        &mut reused,
        long,
        TestShape {
            max_width_px: Some(w),
            halign: HAlign::Left,
            ..shape(fs)
        },
        LineFit::Ellipsis,
    );

    assert_eq!(
        r_fresh.size, r_reused.size,
        "unrelated cached buffers changed the measured size",
    );
    assert_eq!(
        r_fresh.key, r_reused.key,
        "same inputs must map to the same cache key regardless of prior shaping",
    );

    // Truncation actually fired: the ellipsized line is narrower than the
    // full unbounded shape (and fits within the width budget).
    let unbounded = fresh.measure(
        long,
        TestShape {
            halign: HAlign::Left,
            ..shape(fs)
        },
    );
    assert!(
        r_fresh.size.w < unbounded.size.w,
        "expected truncation: ellipsized {} should be < unbounded {}",
        r_fresh.size.w,
        unbounded.size.w,
    );
    assert!(
        r_fresh.size.w <= w + 1.0,
        "ellipsized width {} should fit within budget {w}",
        r_fresh.size.w,
    );
}

/// A continuous font-size zoom over ellipsized text mints a distinct
/// quantized size every frame, so the ellipsis reservation is recomputed
/// throughout. Drive a long sweep of sizes through one budget and assert
/// every one still lands inside it.
#[test]
fn ellipsis_stays_within_budget_under_size_churn() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "the quick brown fox jumps over the lazy dog";
    let width = 60.0;
    for i in 0..261 {
        // Distinct quantized size each iteration (0.1px steps × 64 ≥ 1).
        let fs = 8.0 + i as f32 * 0.1;
        let r = measure_truncated(
            &mut c,
            long,
            TestShape {
                max_width_px: Some(width),
                halign: HAlign::Left,
                ..shape(fs)
            },
            LineFit::Ellipsis,
        );
        assert!(
            r.size.w <= width,
            "size {fs} measured {} against budget {width}",
            r.size.w,
        );
    }
}

/// Inputs that quantize to one key must shape from that key's canonical
/// values, so whichever sub-bucket value inserts first cannot alter the
/// measured extent or glyph positions.
#[test]
fn quantized_key_shaping_is_insertion_order_independent() {
    let text = "canonical text wraps onto more than one aligned line";
    let first = TestShape {
        line_height_px: 19.201,
        max_width_px: Some(101.001),
        halign: HAlign::Right,
        ..shape(16.001)
    };
    let second = TestShape {
        font_size_px: 16.006,
        line_height_px: 19.206,
        max_width_px: Some(101.006),
        ..first
    };

    let mut first_then_second = CosmicMeasure::with_bundled_fonts();
    let a = first_then_second.measure(text, first);
    let a_hit = first_then_second.measure(text, second);
    let mut second_then_first = CosmicMeasure::with_bundled_fonts();
    let b = second_then_first.measure(text, second);
    let b_hit = second_then_first.measure(text, first);

    assert_eq!(a.key, a_hit.key);
    assert_eq!(a.key, b.key);
    assert_eq!(a.key, b_hit.key);
    assert_eq!(a.size, a_hit.size);
    assert_eq!(a.size, b.size);
    assert_eq!(a.size, b_hit.size);
    assert_eq!(a.intrinsic_min, b.intrinsic_min);
    assert_eq!(
        glyph_positions(&first_then_second, a.key),
        glyph_positions(&second_then_first, b.key),
    );
}

#[test]
fn ensure_buffer_exactly_restores_wrap_and_truncation() {
    let text = "restore this shaped buffer after eviction";
    let wrap_params = TestShape {
        line_height_px: 18.003,
        max_width_px: Some(96.003),
        weight: FontWeight::Bold,
        halign: HAlign::Center,
        ..shape(15.003)
    };
    let mut wrap = CosmicMeasure::with_bundled_fonts();
    let original = wrap.measure(text, wrap_params);
    let original_glyphs = glyph_positions(&wrap, original.key);
    wrap.drop_all_buffers();
    assert!(wrap.shaped_run(original.key).is_none());
    wrap.ensure_buffer(TextShapeRequest {
        text,
        key: original.key,
    });
    let restored = wrap.measure(text, wrap_params);
    assert_eq!(restored.size, original.size);
    assert_eq!(restored.intrinsic_min, original.intrinsic_min);
    assert_eq!(glyph_positions(&wrap, restored.key), original_glyphs);

    for fit in [LineFit::Clip, LineFit::Ellipsis] {
        let mut truncated = CosmicMeasure::with_bundled_fonts();
        let params = TestShape {
            max_width_px: Some(84.003),
            ..wrap_params
        };
        let unbounded = truncated.measure(
            text,
            TestShape {
                max_width_px: None,
                halign: HAlign::Auto,
                ..params
            },
        );
        let original = truncated.measure_with_fit(text, params, fit, unbounded.key);
        let original_glyphs = glyph_positions(&truncated, original.key);
        truncated.drop_all_buffers();
        assert!(truncated.shaped_run(original.key).is_none(), "fit: {fit:?}");
        assert!(
            truncated.shaped_run(unbounded.key).is_none(),
            "fit: {fit:?}",
        );

        truncated.ensure_buffer(TextShapeRequest {
            text,
            key: original.key,
        });
        assert!(
            truncated.shaped_run(unbounded.key).is_some(),
            "truncation restoration must rebuild its unbounded probe for {fit:?}",
        );
        let restored = truncated.measure_with_fit(text, params, fit, unbounded.key);
        assert_eq!(restored.size, original.size, "fit: {fit:?}");
        assert_eq!(
            restored.intrinsic_min, original.intrinsic_min,
            "fit: {fit:?}",
        );
        assert_eq!(
            glyph_positions(&truncated, restored.key),
            original_glyphs,
            "fit: {fit:?}",
        );
    }
}

#[test]
fn recycled_buffer_matches_fresh_shape_at_new_width() {
    let text = "recycled cosmic buffers must reshape exactly across a new wrapping width";
    let base = TestShape {
        line_height_px: 18.0,
        max_width_px: Some(180.0),
        weight: FontWeight::Bold,
        halign: HAlign::Right,
        ..shape(15.0)
    };
    let mut recycled = CosmicMeasure::with_bundled_fonts();
    recycled.measure(text, base);
    recycled.drop_all_buffers();
    assert_eq!(recycled.recycle_pool_stats().len, 1);

    let narrow = TestShape {
        max_width_px: Some(72.0),
        ..base
    };
    let actual = recycled.measure(text, narrow);
    assert_eq!(
        recycled.recycle_pool_stats().len,
        0,
        "the new miss must consume the evicted buffer",
    );

    let mut fresh = CosmicMeasure::with_bundled_fonts();
    let expected = fresh.measure(text, narrow);
    assert_eq!(actual.size, expected.size);
    assert_eq!(actual.intrinsic_min, expected.intrinsic_min);
    assert_eq!(
        glyph_positions(&recycled, actual.key),
        glyph_positions(&fresh, expected.key),
    );
}

#[test]
fn recycle_pool_retention_is_bounded() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let pool = c.recycle_pool_stats();
    assert!(pool.capacity >= pool.limit);

    for round in 0..2 {
        for i in 0..pool.limit + 16 {
            c.measure(
                "bounded recycle pool",
                TestShape {
                    line_height_px: 18.0,
                    max_width_px: Some(40.0 + (round * (pool.limit + 16) + i) as f32),
                    halign: HAlign::Left,
                    ..shape(14.0)
                },
            );
        }
        c.drop_all_buffers();
        let after = c.recycle_pool_stats();
        assert_eq!(after.len, pool.limit);
        assert_eq!(after.capacity, pool.capacity);
        assert_eq!(after.limit, pool.limit);
    }
}

/// Shared fixture for the retention tests: `n` distinct cache keys, one
/// per width, all inserted in the current frame.
fn fill_distinct_widths(c: &mut CosmicMeasure, n: u32) -> Vec<TextShapeKey> {
    (0..n)
        .map(|i| {
            c.measure(
                "hello world",
                TestShape {
                    font_size_px: 14.0,
                    line_height_px: 18.0,
                    // Distinct width ⇒ distinct cache key ⇒ a fresh insert.
                    max_width_px: Some(40.0 + i as f32 * 5.0),
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                    halign: HAlign::Left,
                },
            )
            .key
        })
        .collect()
}

fn idle_frames(c: &mut CosmicMeasure, n: u64) {
    for _ in 0..n {
        c.end_frame();
    }
}

/// Retention is by age, not capacity: an untouched entry lives exactly
/// `PROBATION_KEEP_FRAMES` frames past its last touch, and no number of
/// *other* insertions can shorten that.
#[test]
fn probationary_entries_age_out_on_schedule_regardless_of_cache_size() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let keys = fill_distinct_widths(&mut c, 10);
    assert_eq!(c.cache_len(), 10, "ten distinct widths, ten buffers");

    // Inserted during frame 0, so the first four sweeps see a cutoff of
    // 0 (saturated) and keep them; the fifth is the first whose cutoff, 1,
    // is past their stamp.
    idle_frames(&mut c, cosmic::PROBATION_KEEP_FRAMES);
    assert_eq!(
        c.cache_len(),
        10,
        "an entry survives its whole probation window",
    );
    idle_frames(&mut c, 1);
    assert_eq!(c.cache_len(), 0, "one frame past the window, all dropped");
    for key in &keys {
        assert!(c.shaped_run(*key).is_none());
    }

    // Capacity plays no part: a hundred times as many entries age out on
    // exactly the same schedule.
    let mut big = CosmicMeasure::with_bundled_fonts();
    fill_distinct_widths(&mut big, 1000);
    assert_eq!(big.cache_len(), 1000);
    idle_frames(&mut big, cosmic::PROBATION_KEEP_FRAMES);
    assert_eq!(
        big.cache_len(),
        1000,
        "a large working set is not evicted for being large",
    );
    idle_frames(&mut big, 1);
    assert_eq!(big.cache_len(), 0);
}

/// A lookup promotes an entry out of probation and onto the long window.
/// This is the whole scan-resistance mechanism: one-shot drag widths die
/// young, entries something actually came back for do not.
#[test]
fn a_lookup_promotes_an_entry_to_the_protected_window() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let keys = fill_distinct_widths(&mut c, 4);

    // An encoder ensure is a lookup like any other.
    c.ensure_buffer(TextShapeRequest {
        text: "hello world",
        key: keys[0],
    });
    // A layout-side measure of the same key is too.
    let reshaped = c.measure(
        "hello world",
        TestShape {
            line_height_px: 18.0,
            max_width_px: Some(40.0 + 5.0),
            halign: HAlign::Left,
            ..shape(14.0)
        },
    );
    assert_eq!(reshaped.key, keys[1], "same parameters, same key");

    // One frame past probation: the two untouched keys are gone, the two
    // promoted ones are still here — they have 120 frames, not 4.
    idle_frames(&mut c, cosmic::PROBATION_KEEP_FRAMES + 1);
    assert_eq!(c.cache_len(), 2);
    assert!(c.shaped_run(keys[0]).is_some(), "promoted key survives");
    assert!(c.shaped_run(keys[1]).is_some(), "promoted key survives");
    assert!(c.shaped_run(keys[2]).is_none(), "probationary key dropped");
    assert!(c.shaped_run(keys[3]).is_none(), "probationary key dropped");

    // And they last out the protected window, then go.
    idle_frames(
        &mut c,
        cosmic::PROTECTED_KEEP_FRAMES - cosmic::PROBATION_KEEP_FRAMES - 1,
    );
    assert_eq!(c.cache_len(), 2, "still inside the protected window");
    idle_frames(&mut c, 1);
    assert_eq!(c.cache_len(), 0, "one frame past it, dropped");
}

/// The regression the age policy exists to prevent: a live label minting
/// one new key per frame must not cost anything that scales with the size
/// of the cache it lands in, and must never evict the working set around
/// it. Under the old count budget this was a full three-pass sweep every
/// frame — 5.4% of `frame/partial_cpu`.
#[test]
fn steady_key_churn_costs_a_bounded_cache_and_spares_the_working_set() {
    let mut c = CosmicMeasure::with_bundled_fonts();

    // A working set looked up every frame: promoted on the first re-read,
    // and never a candidate afterwards.
    //
    // That access pattern is this unit's contract, not the pipeline's —
    // a real steady-state frame reaches neither the shaper nor the
    // encoder's restore, because the measure cache and the encoded-run
    // cache short-circuit first. `resize_drag_retains_only_the_probation
    // _window` and its neighbours cover what the pipeline actually
    // produces; this one pins the age policy in isolation.
    let working_set = fill_distinct_widths(&mut c, 20);
    // `ensure_buffer` is exactly what the encoder calls; asserting the
    // buffer is present first means an eviction fails here rather than
    // being papered over by the reshape `ensure_buffer` would do.
    let touch_working_set = |c: &mut CosmicMeasure, working_set: &[TextShapeKey]| {
        for key in working_set {
            assert!(
                c.shaped_run(*key).is_some(),
                "a working-set key must never be evicted",
            );
            c.ensure_buffer(TextShapeRequest {
                text: "hello world",
                key: *key,
            });
        }
    };

    let mut lens = Vec::new();
    for frame in 0..60u32 {
        touch_working_set(&mut c, &working_set);
        // One never-seen-before label per frame — a clock, an FPS counter,
        // a progress percentage.
        c.measure(
            &format!("tick {frame}"),
            TestShape {
                line_height_px: 18.0,
                max_width_px: Some(200.0),
                halign: HAlign::Left,
                ..shape(14.0)
            },
        );
        c.end_frame();
        lens.push(c.cache_len());
    }

    // Steady state: the 20 protected keys, plus the counter values from
    // the last PROBATION_KEEP_FRAMES frames — the sweep advances the frame
    // first, so exactly that many stamps sit at or above the cutoff.
    let steady = 20 + cosmic::PROBATION_KEEP_FRAMES as usize;
    assert_eq!(
        lens[10..],
        vec![steady; 50][..],
        "churn must settle at a fixed size, not grow and not thrash",
    );
    for key in &working_set {
        assert!(
            c.shaped_run(*key).is_some(),
            "60 frames of churn must not have touched the working set",
        );
    }
}

/// `ShapedTextRef` is the render-handoff pairing of a shaped-buffer key
/// with its record-store source bytes: `new` pins the pairing against the
/// recorded content hash, `resolve_request` restores the exact request.
#[test]
fn shaped_text_ref_resolves_the_recorded_pair_and_rejects_mismatches() {
    let store = RecordStore::default();
    let recorded = store.record_text(store.intern_str("hi"));
    assert_eq!(recorded.hash, hash_str("hi"));
    let key = TextShapeKey::unbounded(
        recorded.hash,
        16.0,
        19.2,
        FontFamily::Sans,
        FontWeight::Regular,
    );
    let text_ref = ShapedTextRef::new(key, &recorded);
    let other = store.record_text(store.intern_str("bye"));

    {
        let payloads = store.payloads.borrow();
        let interned = payloads.interned_text();
        let request = text_ref.resolve_request(&interned);
        assert_eq!(request.text, "hi");
        assert_eq!(request.key, key);
    }

    // Pairing a key with a different run's source bytes is the logic
    // error the constructor's O(1) hash comparison pins.
    let mismatch = std::panic::catch_unwind(|| ShapedTextRef::new(key, &other));
    assert!(
        mismatch.is_err(),
        "mismatched key/source pairing must panic"
    );
}

/// A run driven through `TextSystem` the way a frame drives it: the
/// intrinsic pass takes the root, the measure pass resolves a width.
/// Returns the bounded key the renderer would replay.
fn drive(text: &mut TextSystem, slot: TextRunSlot, body: &str, width: Option<f32>) -> TextShapeKey {
    let shape = TestShape {
        max_width_px: width,
        halign: HAlign::Left,
        ..ui_shape(14.0)
    };
    text.shape_run(slot, body, shape, TextWrap::Wrap).key
}

/// [`drive`] plus the render half: the encoder's restore on an
/// encoded-cache miss, which is the only thing that promotes a buffer
/// onto the protected window.
///
/// A test that models a *visible* run needs both halves. Layout alone
/// only ever inserts, so a layout-only fixture leaves every buffer on
/// the probation window and would report a bounded cache whether
/// supersession works or not.
fn drive_visible(
    text: &mut TextSystem,
    shaper: &TextShaper,
    slot: TextRunSlot,
    body: &str,
    width: Option<f32>,
) -> TextShapeKey {
    let key = drive(text, slot, body, width);
    shaper.render_ensure(TextShapeRequest { text: body, key });
    key
}

/// End a frame with nothing removed — the steady case.
fn frame_end(text: &mut TextSystem) {
    text.end_frame(&FxHashSet::default());
}

/// A resize drag is the population the probation window exists for, and
/// the one it could not reach before `TextSystem` reported supersession:
/// every frame commits a new whole-pixel width, so every frame mints a
/// bounded key that nothing can ask for again.
///
/// Two things are asserted together because either alone is misleading.
/// The cache must stay bounded by the *probation* window rather than the
/// protected one — 60 frames of 8 runs would otherwise retain every one
/// of the 480 buffers, since a rendered run is looked up on the frame it
/// is inserted and would be promoted there. And the shaping must stay
/// proportional to the drag: one bounded reshape per run per frame is
/// the irreducible cost of the width genuinely changing, but the
/// *unbounded* root must be shaped exactly once per run for the whole
/// drag, because a width drag leaves the unbounded key untouched.
#[test]
fn resize_drag_retains_only_the_probation_window() {
    const RUNS: u32 = 8;
    const FRAMES: u32 = 60;

    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let slots: Vec<TextRunSlot> = (0..RUNS)
        .map(|i| slot(WidgetId::from_hash(("drag", i))))
        .collect();

    // Distinct body per run: `TextShapeKey` carries no widget identity,
    // so eight runs of identical text would share one key and the drag
    // would mint one buffer a frame instead of eight.
    let bodies: Vec<String> = (0..RUNS).map(|i| format!("row {i} of the list")).collect();

    let before = shaper.cache_counts();
    for frame in 0..FRAMES {
        // Whole-pixel steps, so every frame quantizes to a fresh key.
        let width = 120.0 + frame as f32 * 3.0;
        for (s, body) in slots.iter().zip(&bodies) {
            drive_visible(&mut text, &shaper, *s, body, Some(width));
        }
        frame_end(&mut text);
    }
    let counts = shaper.cache_counts() - before;

    // `TextWrap::Wrap` always binds, so a fresh run costs an unbounded
    // root plus a bounded resolve. Afterwards the reuse row answers the
    // root and only the width moves: one bounded reshape per run per
    // later frame, and the root is shaped exactly once for the whole
    // drag.
    assert_eq!(counts.shapes, RUNS * 2 + RUNS * (FRAMES - 1));
    // Every frame but the first supersedes each run's previous width.
    assert_eq!(counts.supersedes, RUNS * (FRAMES - 1));

    // Residency: the live bounded key per run, the buffers still inside
    // their shortened window, and the unbounded root per run. The
    // protected window would have held all 480.
    let resident = shaper.cosmic_cache_len() as u32;
    let ceiling = RUNS * (cosmic::PROBATION_KEEP_FRAMES as u32 + 2) + RUNS;
    assert!(
        resident <= ceiling,
        "drag retained {resident} buffers, over the {ceiling} the \
         probation window allows — supersession is not reaching them",
    );
    assert!(
        resident < RUNS * FRAMES / 4,
        "drag retention is tracking the protected window ({resident})",
    );
}

/// The counterweight: a run that leaves the tree is *not* superseded.
/// Scrolling a row out of view and back within the window must reuse its
/// buffer, which is exactly what the long window is for — so the fix
/// must not shorten it. Told apart from a drag by which signal fires:
/// the slot vanishes rather than moving to a new key.
#[test]
fn scrolled_away_run_keeps_the_protected_window() {
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let wid = WidgetId::from_hash("scrolled row");
    let key = drive_visible(&mut text, &shaper, slot(wid), "row content", Some(200.0));
    frame_end(&mut text);

    // Out of view: the widget stops being recorded, so its reuse row is
    // dropped. Nothing supersedes the key — it may well come back.
    let removed = FxHashSet::from_iter([wid]);
    text.end_frame(&removed);
    for _ in 0..cosmic::PROBATION_KEEP_FRAMES + 2 {
        frame_end(&mut text);
    }
    assert!(
        shaper.has_cosmic_buffer(key),
        "a scrolled-away run must keep the protected window",
    );

    // Back in view inside the window. The bounded buffer — the one the
    // renderer replays — is still resident, so only the unbounded root
    // is reshaped: a wrapped run's root buffer is never promoted (the
    // encoder replays the bounded key), and nothing misses it, because
    // the reuse row caches the root *value* rather than its buffer.
    let before = shaper.cache_counts();
    let again = drive(&mut text, slot(wid), "row content", Some(200.0));
    assert_eq!(again, key);
    assert_eq!(
        (shaper.cache_counts() - before).shapes,
        1,
        "the bounded buffer must survive the scroll — only the root reshapes",
    );

    // What that saved, stated as a contrast: past the protected window
    // the same return costs both shapes.
    for _ in 0..cosmic::PROTECTED_KEEP_FRAMES + 1 {
        frame_end(&mut text);
    }
    assert!(!shaper.has_cosmic_buffer(key), "premise: the window lapsed");
    let before = shaper.cache_counts();
    assert_eq!(drive(&mut text, slot(wid), "row content", Some(200.0)), key);
    assert_eq!(
        (shaper.cache_counts() - before).shapes,
        2,
        "a cold return reshapes root and bounded alike",
    );
}

/// Demotion, not eviction — and that distinction is load-bearing.
/// A label alternating between two widths, or a drag that reverses back
/// through a width it just left, returns inside the probation window and
/// must still hit. Evicting on supersede would turn every reversal into
/// a reshape.
#[test]
fn superseded_key_still_hits_inside_the_probation_window() {
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let s = slot(WidgetId::from_hash("oscillating"));

    let narrow = drive_visible(&mut text, &shaper, s, "alternating label", Some(140.0));
    frame_end(&mut text);
    // Supersedes `narrow`.
    let wide = drive_visible(&mut text, &shaper, s, "alternating label", Some(260.0));
    frame_end(&mut text);
    assert_ne!(narrow, wide);

    // Back to the first width, still inside the shortened window.
    let before = shaper.cache_counts();
    let returned = drive(&mut text, s, "alternating label", Some(140.0));
    let counts = shaper.cache_counts() - before;
    assert_eq!(returned, narrow);
    assert_eq!(
        counts.shapes, 0,
        "a superseded key inside its window must be demoted, not evicted",
    );
    assert!(counts.hits > 0);
}

/// Steady state must be untouched by any of this: a frame redrawing the
/// same runs at the same widths supersedes nothing and shapes nothing.
/// The reuse rows absorb it before the shaper is dispatched at all.
#[test]
fn steady_state_frames_neither_shape_nor_supersede() {
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let slots: Vec<TextRunSlot> = (0..4)
        .map(|i| slot(WidgetId::from_hash(("steady", i))))
        .collect();

    for s in &slots {
        drive(&mut text, *s, "unchanging label", Some(180.0));
    }
    frame_end(&mut text);

    let before = shaper.cache_counts();
    for _ in 0..20 {
        for s in &slots {
            drive(&mut text, *s, "unchanging label", Some(180.0));
        }
        frame_end(&mut text);
    }
    let counts = shaper.cache_counts() - before;
    assert_eq!(counts.shapes, 0, "steady state reshaped");
    assert_eq!(counts.supersedes, 0, "steady state superseded a live key");
    assert_eq!(counts.expiries, 0, "steady state expired a live buffer");
}

/// Typing changes the run itself, so both the unbounded row key and the
/// bounded resolve hanging off it die together — the case a width drag
/// does not cover, since a drag leaves the unbounded key alone.
#[test]
fn typing_supersedes_both_the_root_and_its_bounded_resolve() {
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let s = slot(WidgetId::from_hash("editor"));

    drive_visible(&mut text, &shaper, s, "hell", Some(200.0));
    frame_end(&mut text);

    let before = shaper.cache_counts();
    drive_visible(&mut text, &shaper, s, "hello", Some(200.0));
    let counts = shaper.cache_counts() - before;
    assert_eq!(
        counts.supersedes, 2,
        "a changed run must retire its root *and* its bounded resolve",
    );

    // And the retired pair ages out on the short window, not the long one.
    for _ in 0..cosmic::PROBATION_KEEP_FRAMES + 2 {
        frame_end(&mut text);
    }
    let live = drive(&mut text, s, "hello", Some(200.0));
    assert!(shaper.has_cosmic_buffer(live) || shaper.cosmic_cache_len() > 0);
    assert!(
        shaper.cosmic_cache_len() <= 2,
        "stale keystroke buffers outlived the probation window: {} resident",
        shaper.cosmic_cache_len(),
    );
}

/// Known cost, pinned so it stays known: two slots can hold the same key
/// — a grid of repeated cell text — and supersession is per-slot, so one
/// slot moving on demotes a buffer the other still uses. The worst case
/// is one reshape, never a wrong result, which is why this is accepted
/// rather than refcounted (a per-run map probe every frame to save an
/// occasional reshape is the wrong trade).
#[test]
fn shared_key_demotes_early_and_costs_at_most_one_reshape() {
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let (a, b) = (
        slot(WidgetId::from_hash("cell a")),
        slot(WidgetId::from_hash("cell b")),
    );

    let shared = drive_visible(&mut text, &shaper, a, "—", Some(60.0));
    let same = drive_visible(&mut text, &shaper, b, "—", Some(60.0));
    assert_eq!(shared, same, "identical runs must share one key");
    frame_end(&mut text);

    // Only slot `a` moves on; `b` still displays the shared key.
    drive_visible(&mut text, &shaper, a, "12.5", Some(60.0));
    for _ in 0..cosmic::PROBATION_KEEP_FRAMES + 2 {
        frame_end(&mut text);
    }
    assert!(
        !shaper.has_cosmic_buffer(shared),
        "premise: the shared buffer is demoted by a's move",
    );

    // The cost is bounded at one reshape — `b` recovers on its next ask.
    let before = shaper.cache_counts();
    let recovered = drive(&mut text, b, "—", Some(60.0));
    assert_eq!(recovered, shared);
    assert_eq!(
        (shaper.cache_counts() - before).shapes,
        2,
        "recovery costs one root and one bounded reshape — no more",
    );
}

/// A width-bounded run measures the glyphs it contains, not the distance
/// from the wrap target's left edge to them.
///
/// Cosmic anchors a line wherever alignment and direction put it, so the
/// gap in front of a non-left-aligned run used to count as part of the
/// run's own width — 200 px of "measurement" for 43 px of glyphs. The
/// owner then aligned that full-width block inside its rect, so a hugging
/// container inflated to the whole offer and its damage rect with it.
///
/// The RTL row is the case reachable without an explicit `text_align`:
/// cosmic lays a right-to-left run out from `line_width` leftward
/// (`shape.rs`'s `start_x`) whatever the alignment, so `HAlign::Auto` hit
/// this too.
#[test]
fn a_bounded_run_measures_its_glyphs_not_the_gap_before_them() {
    let mut m = CosmicMeasure::with_bundled_fonts();
    let wrap = 200.0;
    let bounded = |halign| TestShape {
        max_width_px: Some(wrap),
        halign,
        ..ui_shape(16.0)
    };
    for (label, text) in [("LTR", "ab cd"), ("RTL", "مرحبا بالعالم")] {
        let unbounded = m
            .measure(
                text,
                TestShape {
                    max_width_px: None,
                    ..ui_shape(16.0)
                },
            )
            .size
            .w;
        // The run fits the wrap target on one line, so binding a width
        // cannot change how wide the glyphs are — only where cosmic puts
        // them. Every alignment must therefore report the natural width.
        for halign in [
            HAlign::Auto,
            HAlign::Left,
            HAlign::Center,
            HAlign::Right,
            HAlign::Stretch,
        ] {
            let measured = m.measure(text, bounded(halign)).size.w;
            assert_eq!(
                measured, unbounded,
                "{label} {halign:?}: bounded {measured} must equal natural {unbounded}",
            );
        }
        assert!(
            unbounded < wrap - 100.0,
            "{label} must be far narrower than the {wrap} px wrap target, or a \
             measurement that ran to the target would pass by accident; got {unbounded}",
        );
    }
}

/// Caret and hit-test must stay exact inverses now that both correct for
/// the block origin by hand — `cursor_xy` subtracts it, `byte_at_xy` adds
/// it back. Cosmic guarantees the round trip in *its* coordinates, so a
/// sign slip in either correction would break it in ours while each half
/// still looked plausible on its own.
#[test]
fn caret_and_hit_test_round_trip_in_block_local_space() {
    let m = TextShaper::new();
    // Two hard-broken lines of very different widths under right align:
    // the narrow line carries a non-zero block-local offset, which is
    // exactly where an unpaired correction would show up.
    let text = "wwwwww\ni";
    let shape = TestShape {
        max_width_px: Some(300.0),
        halign: HAlign::Right,
        ..ui_shape(16.0)
    };
    for byte in [0usize, 1, 3, 6, 7, 8] {
        assert!(
            text.is_char_boundary(byte),
            "byte {byte} must be a boundary"
        );
        let caret = m.cursor_xy(text, byte, shape);
        // Probe just inside the caret so the hit lands on the glyph the
        // offset belongs to rather than on the boundary between two.
        let hit = m.byte_at_xy(
            text,
            caret.x + 0.5,
            caret.y_top + caret.line_height * 0.5,
            shape,
        );
        assert_eq!(hit, byte, "byte {byte} at x = {} round-trips", caret.x);
    }
}
