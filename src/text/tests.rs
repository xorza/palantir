use crate::common::hash::hash_str;
use crate::text::cosmic::CosmicMeasure;
use crate::text::*;

/// Line-height equal to font size keeps the mono-fallback line
/// height numerically equal to `font_size`, matching the legacy
/// placeholder layout the existing tests pin.
fn lh(font_size: f32) -> f32 {
    font_size
}

fn identity(widget_id: WidgetId, authoring_hash: ContentHash) -> TextRunIdentity {
    TextRunIdentity {
        widget_id,
        ordinal: 0,
        authoring_hash,
    }
}

fn measure_truncated(
    cosmic: &mut CosmicMeasure,
    text: &str,
    params: ShapeParams,
    fit: LineFit,
) -> TextMeasurement {
    let unbounded = cosmic.measure(
        text,
        ShapeParams {
            max_width_px: None,
            halign: HAlign::Auto,
            ..params
        },
    );
    cosmic.measure_truncated(text, params, fit, unbounded.key)
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

fn glyph_positions(cosmic: &CosmicMeasure, key: TextCacheKey) -> Vec<GlyphPosition> {
    cosmic
        .buffer_for(key)
        .expect("shaped buffer must exist")
        .layout_runs()
        .flat_map(|run| {
            run.glyphs.iter().map(move |glyph| GlyphPosition {
                x: glyph.x,
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
        ("empty", "", 16.0, lh(16.0), None, Size::ZERO),
        (
            "unbroken_legacy_short",
            "Hi",
            16.0,
            lh(16.0),
            None,
            Size::new(16.0, 16.0),
        ),
        (
            "unbroken_legacy_long",
            "hello!!",
            16.0,
            lh(16.0),
            None,
            Size::new(56.0, 16.0),
        ),
        (
            "wraps_below_unbroken",
            "12345678",
            16.0,
            lh(16.0),
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
        let r = mono_measure(
            text,
            TextMetrics::new(*fs, *lh_v).unwrap(),
            *max_w,
            LineFit::Wrap,
        );
        assert_eq!(r.size, *expected, "case: {label}");
    }
    // Empty also produces the INVALID sentinel.
    assert!(
        mono_measure(
            "",
            TextMetrics::new(16.0, lh(16.0)).unwrap(),
            None,
            LineFit::Wrap,
        )
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
        ("zero_offset", "hello", 0, 16.0, lh(16.0), 0.0),
        ("empty_string", "", 0, 16.0, lh(16.0), 0.0),
        ("mono_one_char", "abc", 1, 16.0, lh(16.0), 8.0),
        ("mono_two_chars", "abc", 2, 16.0, lh(16.0), 16.0),
        ("mono_three_chars", "abc", 3, 16.0, lh(16.0), 24.0),
        ("lh_independent_short", "abc", 2, 16.0, 16.0, 16.0),
        ("lh_independent_tall", "abc", 2, 16.0, 24.0, 16.0),
    ];
    for (label, text, offset, fs, lh_v, expected) in cases {
        let m = TextShaper::default();
        assert_eq!(
            m.cursor_xy(
                text,
                *offset,
                ShapeParams {
                    font_size_px: *fs,
                    line_height_px: *lh_v,
                    max_width_px: None,
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                    halign: HAlign::Auto
                }
            )
            .x,
            *expected,
            "case: {label}"
        );
    }
}

#[test]
fn cosmic_text_cache_key_distinguishes_line_height() {
    // Pin: at the same font-size, different leadings produce
    // different TextCacheKeys. The renderer caches shaped buffers
    // by key — without this discrimination, a 16/19.2 buffer would
    // be returned for a request that wanted 16/24, mismatching the
    // measured rect against the rasterized glyphs.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let a = c
        .measure(
            "hi",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0 * LINE_HEIGHT_MULT,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .key;
    let b = c
        .measure(
            "hi",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 24.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .key;
    assert_ne!(a, b, "different leading must produce different key");
    assert_ne!(a.lh_q, b.lh_q, "lh_q is the discriminating field");
    assert_eq!(
        a.text_hash,
        hash_str("hi"),
        "direct shaping and authoring use the same canonical text hash",
    );
    // Same call repeated → identical key (cache hit, deterministic).
    let a2 = c
        .measure(
            "hi",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0 * LINE_HEIGHT_MULT,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .key;
    assert_eq!(a, a2);
}

#[test]
fn cosmic_text_family_distinguishes_key_and_metrics() {
    // Pin two independent properties of the two bundled families
    // (Sans — the default proportional — / Mono):
    //
    // 1. Each `FontFamily` resolves, at shape time, to its intended
    //    physical face. Asserted on the resolved family name, not the
    //    measured width, so a coincidental advance match can't masquerade
    //    as the right face.
    // 2. Family enters the cache key (distinct `family_q`), so two runs
    //    differing only by family never collide on one shaped buffer.
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

    let sans = c.measure(
        "MMMM",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    let mono = c.measure(
        "MMMM",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: None,
            family: FontFamily::Mono,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );

    // Discriminants 0 / 1 — distinct, so the shaped-buffer cache slots
    // for the two families never collide.
    assert_eq!(sans.key.family_q, FontFamily::Sans as u8);
    assert_eq!(mono.key.family_q, FontFamily::Mono as u8);
    assert_ne!(sans.key, mono.key, "family must enter the cache key");

    // Cross-check the proportional family against the monospace one:
    // their advances genuinely differ (Inter ≈ 58, JBMono ≈ 39).
    assert!(sans.size.w > 0.0 && sans.size.w.is_finite());
    assert_ne!(
        sans.size.w, mono.size.w,
        "Inter (proportional) and JBMono (monospace) differ for 'MMMM'",
    );
}

#[test]
fn cosmic_text_weight_distinguishes_key_and_metrics() {
    // Pin that `FontWeight` is a live axis end-to-end:
    //
    // 1. Weight enters the cache key (distinct `weight_q`), so a Regular
    //    and a Bold run never collide on one shaped buffer.
    // 2. For the *proportional* Inter, Bold genuinely selects the
    //    bundled bold face — proven by a wider advance. A silent fallback
    //    to Regular (missing/unwired bold face) would shape identical
    //    widths and fail here.
    // 3. For the *monospace*, variable JetBrains Mono, Bold still splits
    //    the cache key (weight instantiated on the `wght` axis) while the
    //    advance stays fixed — monospace keeps its cell width across
    //    weights, so we assert equality there rather than a widening.
    let mut c = CosmicMeasure::with_bundled_fonts();

    let params = |family, weight| ShapeParams {
        font_size_px: 16.0,
        line_height_px: lh(16.0),
        max_width_px: None,
        family,
        weight,
        halign: HAlign::Auto,
    };

    let sans_reg = c.measure("MMMM", params(FontFamily::Sans, FontWeight::Regular));
    let sans_bold = c.measure("MMMM", params(FontFamily::Sans, FontWeight::Bold));

    assert_eq!(sans_reg.key.weight_q, FontWeight::Regular as u8);
    assert_eq!(sans_bold.key.weight_q, FontWeight::Bold as u8);
    assert_ne!(
        sans_reg.key, sans_bold.key,
        "weight must enter the cache key",
    );
    assert!(
        sans_bold.size.w > sans_reg.size.w,
        "Inter Bold ({}) must be wider than Regular ({}) — a smaller-or-equal \
         width means Bold silently fell back to the Regular face",
        sans_bold.size.w,
        sans_reg.size.w,
    );

    let mono_reg = c.measure("MMMM", params(FontFamily::Mono, FontWeight::Regular));
    let mono_bold = c.measure("MMMM", params(FontFamily::Mono, FontWeight::Bold));
    assert_ne!(
        mono_reg.key, mono_bold.key,
        "weight must enter the cache key for the variable mono face too",
    );
    assert_eq!(
        mono_reg.size.w, mono_bold.size.w,
        "monospace advance must be weight-invariant",
    );
}

#[test]
fn prepared_run_caches_per_authoring_hash_only() {
    // The reuse cache is keyed by `(WidgetId, ContentHash)` — different
    // line heights with the *same* hash would collide (same widget
    // id, same hash → cache hit returning the wrong measurement).
    // Authoring-side hash includes line_height_px (pinned in
    // node_hash tests), so callers that change leading must produce
    // a different hash — pin that the measure cache respects the
    // hash distinction.
    let m = TextShaper::default();
    let mut reuse = TextReuseCache::default();
    let wid = WidgetId::from_hash("a");
    let h1 = ContentHash(1);
    let h2 = ContentHash(2);
    let r1 = reuse
        .prepare_run(
            &m,
            identity(wid, h1),
            "hi",
            hash_str("hi"),
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .unwrap()
        .unbounded;
    let r2 = reuse
        .prepare_run(
            &m,
            identity(wid, h2),
            "hi",
            hash_str("hi"),
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 24.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .unwrap()
        .unbounded;
    assert_ne!(
        r1.size.h, r2.size.h,
        "different leading via different hash → distinct cache entries",
    );
    // Re-querying the original hash after refreshing the single identity
    // row must recover the original 16 px measurement.
    let r1_again = reuse
        .prepare_run(
            &m,
            identity(wid, h1),
            "hi",
            hash_str("hi"),
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .unwrap()
        .unbounded;
    assert_eq!(r1.size.h, r1_again.size.h);
}

#[test]
fn prepared_run_refreshes_stale_unbounded_and_bounded_results() {
    let m = TextShaper::default();
    let mut reuse = TextReuseCache::default();
    let wid = WidgetId::from_hash("a");
    let params = ShapeParams {
        font_size_px: 16.0,
        line_height_px: 16.0,
        max_width_px: None,
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
        halign: HAlign::Auto,
    };

    let old = reuse
        .prepare_run(
            &m,
            identity(wid, ContentHash(1)),
            "hi",
            hash_str("hi"),
            params,
        )
        .unwrap();
    assert_eq!(old.unbounded.size, Size::new(16.0, 16.0));
    assert_eq!(
        old.shape_bounded(32.0, HAlign::Auto, LineFit::Wrap)
            .unwrap()
            .size,
        Size::new(16.0, 16.0),
    );

    let current = reuse
        .prepare_run(
            &m,
            identity(wid, ContentHash(2)),
            "abcdefgh",
            hash_str("abcdefgh"),
            params,
        )
        .unwrap();
    assert_eq!(current.unbounded.size, Size::new(64.0, 16.0));
    // Eight 8 px glyphs at 32 px fit four per line: 32 px × two 16 px lines.
    assert_eq!(
        current
            .shape_bounded(32.0, HAlign::Auto, LineFit::Wrap)
            .unwrap()
            .size,
        Size::new(32.0, 32.0),
    );
}

#[test]
fn text_cache_key_validity_is_tagged_by_text_hash() {
    assert!(TextCacheKey::INVALID.is_invalid());
    let real = TextCacheKey {
        text_hash: 1,
        ..TextCacheKey::INVALID
    };
    assert!(!real.is_invalid());
    let zero_hash = TextCacheKey {
        fit_q: LineFit::Ellipsis as u8,
        ..TextCacheKey::INVALID
    };
    assert!(zero_hash.is_invalid());
}

#[test]
fn cursor_xy_cosmic_path_is_monotonic_and_bounded() {
    // With real shaping, caret-x at each byte boundary must be
    // non-decreasing and approach the full-string width at the
    // final offset. Exact pixel values depend on font metrics; we
    // only pin the monotonicity invariant consumers rely on.
    let m = TextShaper::with_bundled_fonts();
    let s = "hello";
    let widths: Vec<f32> = (0..=s.len())
        .map(|i| {
            m.cursor_xy(
                s,
                i,
                ShapeParams {
                    font_size_px: 16.0,
                    line_height_px: 16.0 * LINE_HEIGHT_MULT,
                    max_width_px: None,
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                    halign: HAlign::Auto,
                },
            )
            .x
        })
        .collect();
    assert_eq!(widths[0], 0.0, "caret-x at offset 0 is zero");
    for w in widths.windows(2) {
        assert!(
            w[1] >= w[0] - 0.01,
            "caret-x must be non-decreasing, got {w:?}",
        );
    }
    assert!(
        widths[s.len()] > widths[0],
        "non-empty string has positive width",
    );
}

#[test]
fn byte_at_xy_mono_fallback() {
    // Mono shaper: glyph_w = font_size * 0.5 = 8 px at 16 px font.
    // `byte_at_xy` ignores y on the mono path. Picks the boundary
    // whose prefix-x is closest to `target_x`.
    let m = TextShaper::default();
    let cases: &[(&str, f32, usize)] = &[
        ("origin", 0.0, 0),
        ("first_boundary", 8.0, 1),
        ("mid_glyph_rounds_to_nearer_boundary", 11.0, 1),
        ("mid_glyph_rounds_to_nearer_boundary_other", 13.0, 2),
        ("past_end_clamps", 100.0, 5),
    ];
    for (label, x, expected) in cases {
        let got = m.byte_at_xy(
            "hello",
            *x,
            0.0,
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        );
        assert_eq!(got, *expected, "case: {label}");
    }
}

#[test]
fn byte_at_xy_cosmic_path_monotonic_and_bounded() {
    // Real shaping: caret at the cursor_xy of byte i must hit-test
    // back to a byte close to i; widths sweep monotonically.
    let m = TextShaper::with_bundled_fonts();
    let s = "hello";
    let fs = 16.0;
    let lh_v = fs * LINE_HEIGHT_MULT;
    let probes: Vec<usize> = (0..=s.len())
        .map(|i| {
            let x = m
                .cursor_xy(
                    s,
                    i,
                    ShapeParams {
                        font_size_px: fs,
                        line_height_px: lh_v,
                        max_width_px: None,
                        family: FontFamily::Sans,
                        weight: FontWeight::Regular,
                        halign: HAlign::Auto,
                    },
                )
                .x;
            m.byte_at_xy(
                s,
                x,
                0.0,
                ShapeParams {
                    font_size_px: fs,
                    line_height_px: lh_v,
                    max_width_px: None,
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                    halign: HAlign::Auto,
                },
            )
        })
        .collect();
    // Monotone non-decreasing — hit-test never goes backwards as x grows.
    for w in probes.windows(2) {
        assert!(w[1] >= w[0], "byte_at_xy not monotone: {probes:?}");
    }
    // Past-end x clamps to text.len().
    let past = m.byte_at_xy(
        s,
        10_000.0,
        0.0,
        ShapeParams {
            font_size_px: fs,
            line_height_px: lh_v,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    assert_eq!(past, s.len(), "x past end must clamp to text.len()");
}

#[test]
fn selection_rects_empty_range_is_noop() {
    let m = TextShaper::with_bundled_fonts();
    let mut out: SelectionRects = SelectionRects::new();
    out.push(Rect::new(1.0, 2.0, 3.0, 4.0)); // pre-populate
    m.with_layout(
        "hello",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: 16.0 * LINE_HEIGHT_MULT,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
        |layout| layout.selection_rects(5..5, &mut out),
    )
    .unwrap();
    assert!(
        out.is_empty(),
        "empty range must clear out and emit nothing"
    );
}

#[test]
fn selection_rects_single_line_emits_one_rect() {
    let m = TextShaper::with_bundled_fonts();
    let mut out: SelectionRects = SelectionRects::new();
    m.with_layout(
        "hello",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: 16.0 * LINE_HEIGHT_MULT,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
        |layout| layout.selection_rects(1..4, &mut out),
    )
    .unwrap();
    assert_eq!(out.len(), 1, "single-line range → one rect");
    let r = out[0];
    assert!(r.size.w > 0.0, "rect has positive width");
    assert!(r.size.h > 0.0, "rect has positive height");
    // Origin pinned at y = 0 — first (and only) visual line.
    assert!(
        r.min.y.abs() < 0.5,
        "single line starts at y≈0, got {}",
        r.min.y
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

    let m = TextShaper::with_bundled_fonts();
    let cases = [
        Case {
            label: "hard_breaks",
            text: "abc\ndef\nghi",
            range: 0..11,
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
        let params = ShapeParams {
            font_size_px: 16.0,
            line_height_px: 16.0 * LINE_HEIGHT_MULT,
            max_width_px: case.max_width_px,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        };
        let mut expected = Vec::new();
        m.with_layout(case.text, params, |layout| {
            let buffer = layout.buffer.unwrap();
            let start = cursor_from_byte(case.text, case.range.start);
            let end = cursor_from_byte(case.text, case.range.end);
            for run in buffer.layout_runs() {
                expected.extend(
                    run.highlight(start, end)
                        .map(|(x, w)| Rect::new(x, run.line_top, w, run.line_height)),
                );
            }
        })
        .unwrap();

        let mut actual = SelectionRects::new();
        m.with_layout(case.text, params, |layout| {
            layout.selection_rects(case.range, &mut actual);
        })
        .unwrap();
        assert_eq!(
            actual.as_slice(),
            expected.as_slice(),
            "case: {}",
            case.label
        );
    }
}

#[test]
fn cursor_byte_round_trip_multiline() {
    // `cursor_from_byte` and `cursor_to_byte` must invert each other
    // across line breaks. Offsets sampled at every byte position of a
    // 3-line string with varying line lengths.
    let text = "ab\ncde\nfg";
    for off in 0..=text.len() {
        let cur = cursor_from_byte(text, off);
        let back = cursor_to_byte(text, cur);
        assert_eq!(
            back, off,
            "round-trip failed at offset {off}, cursor={cur:?}"
        );
    }
    // Line counts: offsets 0..=2 → line 0; 3..=6 → line 1; 7..=9 → line 2.
    assert_eq!(cursor_from_byte(text, 0).line, 0);
    assert_eq!(cursor_from_byte(text, 2).line, 0);
    assert_eq!(cursor_from_byte(text, 3).line, 1);
    assert_eq!(cursor_from_byte(text, 6).line, 1);
    assert_eq!(cursor_from_byte(text, 7).line, 2);
    assert_eq!(cursor_from_byte(text, 9).line, 2);
}

#[test]
fn cursor_xy_multiline_y_top_advances_per_line() {
    // Two-line buffer: caret on line 1 must have y_top > caret on line 0,
    // and the delta must be ≈ line_height. Pins multi-line caret routing
    // through cosmic's layout_runs.
    let m = TextShaper::with_bundled_fonts();
    let fs = 16.0;
    let lh_v = fs * LINE_HEIGHT_MULT;
    let p0 = m.cursor_xy(
        "abc\ndef",
        0,
        ShapeParams {
            font_size_px: fs,
            line_height_px: lh_v,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    let p1 = m.cursor_xy(
        "abc\ndef",
        4,
        ShapeParams {
            font_size_px: fs,
            line_height_px: lh_v,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
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
    let r = c.measure(
        "",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: 16.0 * LINE_HEIGHT_MULT,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    assert_eq!(r.size, Size::ZERO);
    assert!(r.key.is_invalid());
    assert_eq!(r.intrinsic_min, 0.0);
    // `buffer_for(INVALID)` must return None — even after measuring,
    // no buffer was cached for the empty input.
    assert!(c.buffer_for(r.key).is_none());

    let shaper = TextShaper::mono();
    let calls = shaper.measure_calls();
    let r = shaper
        .measure(
            "",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0 * LINE_HEIGHT_MULT,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .expect("empty text has valid shaping parameters");
    assert_eq!(r.size, Size::ZERO);
    assert_eq!(r.intrinsic_min, 0.0);
    assert_eq!(shaper.measure_calls(), calls);
}

#[test]
fn invalid_metrics_never_dispatch_or_enter_direct_shaping_caches() {
    use crate::primitives::approx::EPS;

    let cases = [
        ("zero font", 0.0, 16.0, ShapeParamsError::InvalidFontSize),
        (
            "negative font",
            -1.0,
            16.0,
            ShapeParamsError::InvalidFontSize,
        ),
        (
            "sub-epsilon font",
            EPS * 0.5,
            16.0,
            ShapeParamsError::InvalidFontSize,
        ),
        ("epsilon font", EPS, 16.0, ShapeParamsError::InvalidFontSize),
        (
            "NaN font",
            f32::NAN,
            16.0,
            ShapeParamsError::InvalidFontSize,
        ),
        (
            "infinite font",
            f32::INFINITY,
            16.0,
            ShapeParamsError::InvalidFontSize,
        ),
        (
            "zero line height",
            16.0,
            0.0,
            ShapeParamsError::InvalidLineHeight,
        ),
        (
            "negative line height",
            16.0,
            -1.0,
            ShapeParamsError::InvalidLineHeight,
        ),
        (
            "sub-epsilon line height",
            16.0,
            EPS * 0.5,
            ShapeParamsError::InvalidLineHeight,
        ),
        (
            "epsilon line height",
            16.0,
            EPS,
            ShapeParamsError::InvalidLineHeight,
        ),
        (
            "NaN line height",
            16.0,
            f32::NAN,
            ShapeParamsError::InvalidLineHeight,
        ),
        (
            "infinite line height",
            16.0,
            f32::INFINITY,
            ShapeParamsError::InvalidLineHeight,
        ),
    ];
    let mono = TextShaper::mono();
    let cosmic = TextShaper::with_bundled_fonts();
    for (label, font_size_px, line_height_px, expected_error) in cases {
        let params = ShapeParams {
            font_size_px,
            line_height_px,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        };
        for shaper in [&mono, &cosmic] {
            let calls = shaper.measure_calls();
            let error = shaper.measure("hi", params).unwrap_err();
            assert_eq!(error, expected_error, "{label}");
            assert_eq!(
                shaper.measure_calls(),
                calls,
                "{label}: invalid metrics reached a shaping dispatch",
            );
        }
    }
}

#[test]
fn prepared_run_rejects_invalid_inputs_before_dispatch() {
    use crate::primitives::approx::EPS;

    let shaper = TextShaper::with_bundled_fonts();
    for (index, fit) in [LineFit::Wrap, LineFit::Clip, LineFit::Ellipsis]
        .into_iter()
        .enumerate()
    {
        let mut reuse = TextReuseCache::default();
        let widget_id = WidgetId::from_hash(("invalid metrics", index));
        let identity = identity(widget_id, ContentHash(index as u64));
        let invalid_metrics = ShapeParams {
            font_size_px: EPS * 0.5,
            line_height_px: 16.0,
            max_width_px: Some(40.0),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Center,
        };
        let calls = shaper.measure_calls();

        let error = reuse
            .prepare_run(&shaper, identity, "hi", 1, invalid_metrics)
            .unwrap_err();
        assert_eq!(error, ShapeParamsError::InvalidFontSize, "fit={fit:?}",);
        assert!(
            !reuse.has_entry(widget_id, 0),
            "fit={fit:?}: invalid metrics entered the reuse cache",
        );
        assert_eq!(
            shaper.measure_calls(),
            calls,
            "fit={fit:?}: invalid metrics reached a shaping dispatch",
        );

        let prepared = reuse
            .prepare_run(
                &shaper,
                identity,
                "hi",
                1,
                ShapeParams {
                    font_size_px: 16.0,
                    line_height_px: 16.0,
                    max_width_px: None,
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                    halign: HAlign::Auto,
                },
            )
            .unwrap();
        let calls = shaper.measure_calls();
        let error = prepared
            .shape_bounded(f32::NAN, HAlign::Center, fit)
            .unwrap_err();
        assert_eq!(error, ShapeParamsError::InvalidMaxWidth, "fit={fit:?}");
        assert_eq!(
            shaper.measure_calls(),
            calls,
            "fit={fit:?}: invalid width reached a shaping dispatch",
        );
    }
}

#[test]
fn bounded_width_requires_a_finite_nonnegative_value() {
    let base = ShapeParams {
        font_size_px: 16.0,
        line_height_px: 19.2,
        max_width_px: None,
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
        halign: HAlign::Auto,
    };
    assert!(base.validated().is_ok(), "None is the unbounded form");
    assert!(
        ShapeParams {
            max_width_px: Some(0.0),
            ..base
        }
        .validated()
        .is_ok(),
        "zero is a valid bounded width",
    );
    for (label, width) in [
        ("negative", -1.0),
        ("NaN", f32::NAN),
        ("positive infinity", f32::INFINITY),
        ("negative infinity", f32::NEG_INFINITY),
    ] {
        let params = ShapeParams {
            max_width_px: Some(width),
            ..base
        };
        assert_eq!(
            params.validated().unwrap_err(),
            ShapeParamsError::InvalidMaxWidth,
            "{label}",
        );
        let shaper = TextShaper::with_bundled_fonts();
        let calls = shaper.measure_calls();
        assert_eq!(
            shaper.measure("hi", params).unwrap_err(),
            ShapeParamsError::InvalidMaxWidth,
            "{label}",
        );
        assert_eq!(shaper.measure_calls(), calls, "{label}");
    }
}

#[test]
fn above_epsilon_metrics_survive_cache_key_canonicalization() {
    use crate::primitives::approx::EPS;

    let mut cosmic = CosmicMeasure::with_bundled_fonts();
    let result = cosmic.measure(
        "x",
        ShapeParams {
            font_size_px: EPS * 2.0,
            line_height_px: EPS * 2.0,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    assert!(!result.key.is_invalid());
    assert_eq!(result.key.size_q, 1);
    assert_eq!(result.key.lh_q, 1);
    assert!(cosmic.buffer_for(result.key).is_some());
}

#[test]
fn cosmic_intrinsic_min_tracks_longest_word() {
    // `intrinsic_min` = width of the widest unbreakable run. For a
    // multi-word string, it must (a) be strictly positive, (b) be
    // strictly less than the unbroken total (multiple words present),
    // and (c) match the standalone measurement of the longest word
    // within shaping tolerance — that's the wrap floor downstream
    // layout pins as the "can't break below this" guarantee.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let full = c.measure(
        "hello world hi",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: 16.0 * LINE_HEIGHT_MULT,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    let hello = c.measure(
        "hello",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: 16.0 * LINE_HEIGHT_MULT,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    let world = c.measure(
        "world",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: 16.0 * LINE_HEIGHT_MULT,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    let hi = c.measure(
        "hi",
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: 16.0 * LINE_HEIGHT_MULT,
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    let longest_w = hello.size.w.max(world.size.w).max(hi.size.w);
    assert!(
        full.intrinsic_min > hi.size.w,
        "must exceed the shortest word"
    );
    assert!(
        full.intrinsic_min < full.size.w,
        "multi-word intrinsic_min ({}) must be < total width ({})",
        full.intrinsic_min,
        full.size.w,
    );
    // Within shaping tolerance — kerning around space glyphs can
    // shift the in-run word width a couple of px vs the standalone
    // measurement, so allow ±10%.
    let rel_err = (full.intrinsic_min - longest_w).abs() / longest_w;
    assert!(
        rel_err < 0.15,
        "intrinsic_min ({}) must ≈ longest-word width ({}), rel_err = {}",
        full.intrinsic_min,
        longest_w,
        rel_err,
    );
    // Single-word input: intrinsic_min ≈ size.w. size.w is the
    // last glyph's (x + w) ceil'd; intrinsic_min sums glyph widths.
    // The two differ by sub-pixel kerning / ceil rounding — allow 2 px.
    assert!(
        (hello.intrinsic_min - hello.size.w).abs() < 2.0,
        "single-word: intrinsic_min ({}) ≈ size.w ({})",
        hello.intrinsic_min,
        hello.size.w,
    );
}

#[test]
fn cache_key_collapses_halign_when_unbounded() {
    // Optimization in `cosmic::key_for`: halign only affects shaped
    // glyph positions when there's a wrap target, so without one the
    // key folds non-Auto halign down to Auto. Two unbounded measures
    // with different halign must therefore produce identical keys —
    // single-line callers don't pay an N-way cache split.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let auto = c
        .measure(
            "hi",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .key;
    let right = c
        .measure(
            "hi",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Right,
            },
        )
        .key;
    let center = c
        .measure(
            "hi",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Center,
            },
        )
        .key;
    assert_eq!(auto, right, "unbounded: halign collapses to Auto in key");
    assert_eq!(auto, center, "unbounded: halign collapses to Auto in key");
    // With a wrap target the keys must diverge — per-line align now
    // affects glyph positions in the shaped buffer.
    let auto_w = c
        .measure(
            "hi",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: Some(200.0),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .key;
    let right_w = c
        .measure(
            "hi",
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: Some(200.0),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Right,
            },
        )
        .key;
    assert_ne!(auto_w, right_w, "wrap-bounded: halign must enter the key");
}

#[test]
fn prepared_bounded_cache_keys_width_and_halign() {
    let m = TextShaper::with_bundled_fonts();
    let mut reuse = TextReuseCache::default();
    let wid = WidgetId::from_hash("w");
    let hash = ContentHash(7);
    let params = ShapeParams {
        font_size_px: 16.0,
        line_height_px: 16.0,
        max_width_px: None,
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
        halign: HAlign::Auto,
    };
    reuse
        .prepare_run(&m, identity(wid, hash), "hi", hash_str("hi"), params)
        .unwrap();
    let baseline = m.measure_calls();

    reuse
        .prepare_run(&m, identity(wid, hash), "hi", hash_str("hi"), params)
        .unwrap()
        .shape_bounded(200.0, HAlign::Left, LineFit::Wrap)
        .unwrap();
    let after_left = m.measure_calls();
    assert_eq!(after_left, baseline + 1, "first wrap shape must dispatch");

    reuse
        .prepare_run(&m, identity(wid, hash), "hi", hash_str("hi"), params)
        .unwrap()
        .shape_bounded(200.0, HAlign::Left, LineFit::Wrap)
        .unwrap();
    assert_eq!(
        m.measure_calls(),
        after_left,
        "identical wrap call must hit cache"
    );

    reuse
        .prepare_run(&m, identity(wid, hash), "hi", hash_str("hi"), params)
        .unwrap()
        .shape_bounded(200.0, HAlign::Right, LineFit::Wrap)
        .unwrap();
    assert_eq!(
        m.measure_calls(),
        after_left + 1,
        "halign change at same target must bust wrap reuse",
    );

    reuse
        .prepare_run(&m, identity(wid, hash), "hi", hash_str("hi"), params)
        .unwrap()
        .shape_bounded(201.0, HAlign::Right, LineFit::Wrap)
        .unwrap();
    assert_eq!(
        m.measure_calls(),
        after_left + 2,
        "width change must bust wrap reuse",
    );
}

#[test]
fn sweep_removed_evicts_reuse_entries() {
    // `LayoutEngine::sweep_removed` calls this with the per-window removed
    // set. Pin: removed ids vanish while surviving ids stay.
    let m = TextShaper::default();
    let mut reuse = TextReuseCache::default();
    let a = WidgetId::from_hash("a");
    let b = WidgetId::from_hash("b");
    reuse
        .prepare_run(
            &m,
            identity(a, ContentHash(1)),
            "hi",
            hash_str("hi"),
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .unwrap();
    reuse
        .prepare_run(
            &m,
            identity(b, ContentHash(2)),
            "yo",
            hash_str("yo"),
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: 16.0,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        )
        .unwrap();
    assert!(reuse.has_entry(a, 0));
    assert!(reuse.has_entry(b, 0));
    let removed: FxHashSet<WidgetId> = FxHashSet::from_iter([a]);
    reuse.sweep_removed(&removed);
    assert!(!reuse.has_entry(a, 0), "removed widget's entry evicted");
    assert!(reuse.has_entry(b, 0), "surviving widget's entry kept");
    // Empty removed set is a no-op (early return path).
    reuse.sweep_removed(&FxHashSet::default());
    assert!(reuse.has_entry(b, 0));
}

/// Right-aligned multi-line buffer: caret at byte 4 ("abc\n|") lands
/// on the empty second line. Cosmic's per-line halign offset only
/// shifts existing glyphs, so an empty line has `line_w = 0` and
/// the naive `unwrap_or(run.line_w)` reports `x = 0` (left edge).
/// Post-fix the empty-line branch routes through `empty_line_x`,
/// putting the caret at the right edge of the wrap target.
#[test]
fn cursor_xy_on_empty_line_respects_right_align() {
    let m = TextShaper::with_bundled_fonts();
    let text = "abc\n";
    let wrap = 200.0;
    let font = 16.0;
    let line_h = font * LINE_HEIGHT_MULT;
    // `cursor_xy` calls `with_buffer` which in turn drives
    // `measure` end-to-end (unbounded + wrap-shape), so no
    // pre-prime is needed — the shaper builds whatever cache
    // entry it needs on first hit.
    let pos = m.cursor_xy(
        text,
        text.len(),
        ShapeParams {
            font_size_px: font,
            line_height_px: line_h,
            max_width_px: Some(wrap),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Right,
        },
    );
    assert!(
        (pos.x - wrap).abs() < 0.5,
        "right-aligned caret on empty trailing line must sit at \
         the wrap target ({wrap}); got x = {}",
        pos.x,
    );
    // And the left-aligned counterpart still anchors at zero —
    // sanity-pins the helper isn't accidentally always returning
    // the right edge.
    let pos_left = m.cursor_xy(
        text,
        text.len(),
        ShapeParams {
            font_size_px: font,
            line_height_px: line_h,
            max_width_px: Some(wrap),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Left,
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
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: Some(w),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
        LineFit::Ellipsis,
    );
    // Precondition: the natural single line genuinely overflows `w`.
    let full = c.measure(
        long,
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
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
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: Some(0.0),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
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
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: Some(w),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    assert_ne!(
        elided.key, wrapped.key,
        "elision and wrap must key distinct cache slots at the same width",
    );
}

#[test]
fn cosmic_ellipsis_short_text_not_truncated() {
    // A label that already fits the cap is shaped whole — no spurious
    // ellipsis, width matches the natural measurement.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let short = "ok";
    let natural = c.measure(
        short,
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    let elided = measure_truncated(
        &mut c,
        short,
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: Some(200.0),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
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
    let natural = c.measure(
        label,
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    for fit in [false, true] {
        let m = measure_truncated(
            &mut c,
            label,
            ShapeParams {
                font_size_px: 16.0,
                line_height_px: lh(16.0),
                max_width_px: Some(cap),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Center,
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
    let full = c.measure(
        long,
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
    );
    assert!(
        full.size.w > w,
        "precondition: natural line ({}) must overflow the cap ({w})",
        full.size.w,
    );
    let clipped = measure_truncated(
        &mut c,
        long,
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: Some(w),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
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
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: Some(w),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
        },
        LineFit::Ellipsis,
    );
    // Clip, ellipsis, and wrap each bake a distinct buffer at the same width.
    let wrapped = c.measure(
        long,
        ShapeParams {
            font_size_px: 16.0,
            line_height_px: lh(16.0),
            max_width_px: Some(w),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Auto,
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
        "bounded keys reuse the authoring-time text hash",
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
    let metrics = TextMetrics::new(16.0, lh(16.0)).unwrap();
    let elided = mono_measure(long, metrics, Some(w), LineFit::Ellipsis);
    assert_eq!(elided.size.w, w, "elided mono caps at the width");
    assert_eq!(elided.size.h, lh(16.0), "elided mono is one line");
    assert_eq!(elided.intrinsic_min, 0.0, "elided mono has zero floor");
    let wrapped = mono_measure(long, metrics, Some(w), LineFit::Wrap);
    assert!(wrapped.size.h > lh(16.0), "wrap grows height across lines");
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
        ShapeParams {
            font_size_px: fs,
            line_height_px: lh(fs),
            max_width_px: Some(w),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Left,
        },
        LineFit::Ellipsis,
    );

    // Reused measurer: populate unrelated unbounded, truncated, and ellipsis
    // cache entries first, then measure the identical target.
    let mut reused = CosmicMeasure::with_bundled_fonts();
    measure_truncated(
        &mut reused,
        "a considerably longer string that grows the probe buffer capacity",
        ShapeParams {
            font_size_px: 20.0,
            line_height_px: lh(20.0),
            max_width_px: Some(220.0),
            family: FontFamily::Mono,
            weight: FontWeight::Regular,
            halign: HAlign::Left,
        },
        LineFit::Ellipsis,
    );
    measure_truncated(
        &mut reused,
        "short",
        ShapeParams {
            font_size_px: 10.0,
            line_height_px: lh(10.0),
            max_width_px: Some(30.0),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Left,
        },
        LineFit::Clip,
    );
    let r_reused = measure_truncated(
        &mut reused,
        long,
        ShapeParams {
            font_size_px: fs,
            line_height_px: lh(fs),
            max_width_px: Some(w),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Left,
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
        ShapeParams {
            font_size_px: fs,
            line_height_px: lh(fs),
            max_width_px: None,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            halign: HAlign::Left,
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

/// The ellipsis-advance memo is keyed on quantized size, so a continuous
/// font-size zoom over ellipsized text would grow it without bound. Drive
/// far more distinct sizes than the cap and assert it stays bounded (the
/// clear-on-overflow path), while still returning correct widths.
#[test]
fn ellipsis_cache_bounded_under_size_churn() {
    use crate::text::cosmic::ELLIPSIS_CACHE_CAP;

    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "the quick brown fox jumps over the lazy dog";
    for i in 0..(ELLIPSIS_CACHE_CAP * 2 + 5) {
        // Distinct quantized size each iteration (0.1px steps × 64 ≥ 1).
        let fs = 8.0 + i as f32 * 0.1;
        let r = measure_truncated(
            &mut c,
            long,
            ShapeParams {
                font_size_px: fs,
                line_height_px: lh(fs),
                max_width_px: Some(60.0),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Left,
            },
            LineFit::Ellipsis,
        );
        assert!(r.size.w <= 61.0, "still truncates to budget at size {fs}");
    }
    assert!(
        c.ellipsis_cache_len() <= ELLIPSIS_CACHE_CAP,
        "ellipsis cache must stay bounded ({} > cap {ELLIPSIS_CACHE_CAP})",
        c.ellipsis_cache_len(),
    );
}

/// Inputs that quantize to one key must shape from that key's canonical
/// values, so whichever sub-bucket value inserts first cannot alter the
/// measured extent or glyph positions.
#[test]
fn quantized_key_shaping_is_insertion_order_independent() {
    let text = "canonical text wraps onto more than one aligned line";
    let first = ShapeParams {
        font_size_px: 16.001,
        line_height_px: 19.201,
        max_width_px: Some(101.001),
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
        halign: HAlign::Right,
    };
    let second = ShapeParams {
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
    let text = "restore this shaped buffer after ordinary LRU eviction";
    let wrap_params = ShapeParams {
        font_size_px: 15.003,
        line_height_px: 18.003,
        max_width_px: Some(96.003),
        family: FontFamily::Sans,
        weight: FontWeight::Bold,
        halign: HAlign::Center,
    };
    let mut wrap = CosmicMeasure::with_bundled_fonts();
    let original = wrap.measure(text, wrap_params);
    let original_glyphs = glyph_positions(&wrap, original.key);
    wrap.end_frame_evict(0);
    assert!(wrap.buffer_for(original.key).is_none());
    wrap.ensure_buffer(text, original.key);
    let restored = wrap.measure(text, wrap_params);
    assert_eq!(restored.size, original.size);
    assert_eq!(restored.intrinsic_min, original.intrinsic_min);
    assert_eq!(glyph_positions(&wrap, restored.key), original_glyphs);

    for fit in [LineFit::Clip, LineFit::Ellipsis] {
        let mut truncated = CosmicMeasure::with_bundled_fonts();
        let params = ShapeParams {
            max_width_px: Some(84.003),
            ..wrap_params
        };
        let unbounded = truncated.measure(
            text,
            ShapeParams {
                max_width_px: None,
                halign: HAlign::Auto,
                ..params
            },
        );
        let original = truncated.measure_truncated(text, params, fit, unbounded.key);
        let original_glyphs = glyph_positions(&truncated, original.key);
        truncated.end_frame_evict(0);
        assert!(truncated.buffer_for(original.key).is_none(), "fit: {fit:?}");
        assert!(
            truncated.buffer_for(unbounded.key).is_none(),
            "fit: {fit:?}",
        );

        truncated.ensure_buffer(text, original.key);
        assert!(
            truncated.buffer_for(unbounded.key).is_some(),
            "truncation restoration must rebuild its unbounded probe for {fit:?}",
        );
        let restored = truncated.measure_truncated(text, params, fit, unbounded.key);
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
    let base = ShapeParams {
        font_size_px: 15.0,
        line_height_px: 18.0,
        max_width_px: Some(180.0),
        family: FontFamily::Sans,
        weight: FontWeight::Bold,
        halign: HAlign::Right,
    };
    let mut recycled = CosmicMeasure::with_bundled_fonts();
    recycled.measure(text, base);
    recycled.end_frame_evict(0);
    assert_eq!(recycled.recycle_pool_stats().len, 1);

    let narrow = ShapeParams {
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
                ShapeParams {
                    font_size_px: 14.0,
                    line_height_px: 18.0,
                    max_width_px: Some(40.0 + (round * (pool.limit + 16) + i) as f32),
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                    halign: HAlign::Left,
                },
            );
        }
        c.end_frame_evict(0);
        let after = c.recycle_pool_stats();
        assert_eq!(after.len, pool.limit);
        assert_eq!(after.capacity, pool.capacity);
        assert_eq!(after.limit, pool.limit);
    }
}

/// Encoder ensures and layout hits both refresh recency. Touch the oldest
/// entry through `ensure_buffer`, then retain exactly two entries and verify
/// that refreshed entry and the newest insertion survive.
#[test]
fn end_frame_evict_retains_exact_most_recent_entries() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let mut keys = Vec::new();
    for i in 0..10u32 {
        // Distinct width per frame ⇒ distinct cache key ⇒ a fresh insert
        // stamped with that frame's generation.
        let r = c.measure(
            "hello world",
            ShapeParams {
                font_size_px: 14.0,
                line_height_px: lh(18.0),
                max_width_px: Some(40.0 + i as f32 * 5.0),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Left,
            },
        );
        keys.push(r.key);
    }
    assert_eq!(c.cache_len(), 10, "ten distinct widths, ten buffers");

    c.ensure_buffer("hello world", keys[0]);
    c.end_frame_evict(2);

    assert_eq!(c.cache_len(), 2, "eviction must enforce the exact budget");
    assert!(
        c.buffer_for(keys[0]).is_some(),
        "encoder ensure must refresh the oldest key's recency",
    );
    assert!(c.buffer_for(keys[9]).is_some(), "newest insertion kept");
    for evicted in [1usize, 2, 5, 7, 8] {
        assert!(
            c.buffer_for(keys[evicted]).is_none(),
            "older key {evicted} evicted",
        );
    }
}

/// Below budget the cache is left completely untouched — the no-regression
/// guarantee for bounded multi-size rotation (`frame/resizing_cpu`), whose
/// working set never crosses the budget and so must never reshape.
#[test]
fn end_frame_evict_is_noop_under_budget() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let mut keys = Vec::new();
    for i in 0..4u32 {
        let r = c.measure(
            "rotation",
            ShapeParams {
                font_size_px: 14.0,
                line_height_px: lh(18.0),
                max_width_px: Some(100.0 + i as f32 * 20.0),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Left,
            },
        );
        keys.push(r.key);
    }
    c.end_frame_evict(64);
    assert_eq!(c.cache_len(), 4, "under-budget eviction is a no-op");
    for k in &keys {
        assert!(c.buffer_for(*k).is_some(), "every rotation width retained");
    }
}
