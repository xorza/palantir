use super::*;
use crate::common::hash;
use crate::text::font_scope::internals::INTER;
use crate::text::font_source::FontSource;
use crate::text::probe::Caret;
use crate::text::render::RunPlacement;
use std::ops;

/// `cursor_xy(...).x` on the mono fallback: each ASCII byte is
/// `font_size * 0.5` wide, independent of `line_height`. Empty text and zero
/// offset give zero.
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
        // Byte 3 of "éa" ends after `a`: two chars in, not three bytes.
        ("multibyte_counts_chars", "éa", 3, 16.0, 16.0, 16.0),
    ];
    let m = TextShaper::test_mono();
    for (label, text, offset, fs, lh_v, expected) in cases {
        assert_eq!(
            m.cursor_xy(text, *offset, shape(*fs).leading(*lh_v)).x,
            *expected,
            "case: {label}"
        );
    }
}

/// Caret x at each byte boundary advances with the run's reading direction,
/// from the edge the direction starts on.
///
/// LTR, bundled Inter at 16 px: advances h 9.4609375, e 9.328125, l 3.875,
/// o 9.59375; the carets are their running sums.
///
/// RTL: no bundled face covers Hebrew, so each letter is the 10.5 px
/// missing-glyph box. Byte 0 sits at the right edge, 4 × 10.5 = 42, and the
/// caret walks left. Letters are two bytes, so odd offsets resolve to the
/// caret after the letter; a glyph-start scan would be a letter off.
#[test]
fn cursor_xy_walks_with_the_paragraph_direction() {
    let shaper = TextShaper::new();
    let shape = ui_shape(16.0);
    let carets = |text: &str| -> Vec<f32> {
        (0..=text.len())
            .map(|i| shaper.cursor_xy(text, i, shape).x)
            .collect()
    };

    // Each advance is a multiple of 1/128, so every sum is exact in f32.
    let (h, e, l, o) = (9.4609375, 9.328125, 3.875, 9.59375);
    assert_eq!(
        carets("hello"),
        [
            0.0,
            h,
            h + e,
            h + e + l,
            h + e + 2.0 * l,
            h + e + 2.0 * l + o
        ],
    );

    let rtl_text = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}";
    assert_eq!(
        carets(rtl_text),
        [42.0, 31.5, 31.5, 21.0, 21.0, 10.5, 10.5, 0.0, 0.0],
    );
    // The extent spans every glyph, not just the last in the array (leftmost
    // in RTL), or the run would measure one letter wide. One letter alone
    // rounds 10.5 up to 11.
    assert_eq!(shaper.measure(rtl_text, shape).extent.size.w, 42.0);
    assert_eq!(shaper.measure("\u{5e9}", shape).extent.size.w, 11.0);
}

#[test]
fn byte_at_xy_mono_fallback() {
    // Mono shaper: glyph_w = 8 px at 16 px font; `byte_at_xy` ignores y and
    // picks the boundary whose prefix-x is closest to `target_x`.
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
    // Two-byte chars: x = 16 is after the second `é`, byte 4, not byte 2.
    assert_eq!(m.byte_at_xy("ééé", 16.0, 0.0, shape(16.0)), 4);
}

/// Real shaping: each caret x of "hello" hits back to its own offset, and
/// either side of a glyph's midpoint picks the nearer edge (`h` spans
/// 0 to 9.4609375, midpoint 4.73046875). An x past the end clamps.
#[test]
fn byte_at_xy_cosmic_path_hits_each_caret_and_clamps() {
    let m = TextShaper::new();
    let s = "hello";
    let shape = ui_shape(16.0);
    let probes: Vec<usize> = (0..=s.len())
        .map(|i| m.byte_at_xy(s, m.cursor_xy(s, i, shape).x, 0.0, shape))
        .collect();
    assert_eq!(probes, [0, 1, 2, 3, 4, 5]);
    assert_eq!(m.byte_at_xy(s, 4.7, 0.0, shape), 0);
    assert_eq!(m.byte_at_xy(s, 4.8, 0.0, shape), 1);
    assert_eq!(
        m.byte_at_xy(s, 10_000.0, 0.0, shape),
        s.len(),
        "x past end must clamp to text.len()",
    );
}

/// An empty range emits nothing and leaves the caller's buffer alone;
/// clearing belongs to the caller (`resolve_geometry`).
#[test]
fn selection_rects_empty_range_emits_nothing_and_touches_no_buffer() {
    let m = TextShaper::new();
    let mut out: Vec<Rect> = Vec::new();
    let pre = Rect::new(1.0, 2.0, 3.0, 4.0);
    out.push(pre); // pre-populate
    m.probe_layout("hello", ui_shape(16.0), |layout| {
        assert_eq!(
            layout.text_hash(),
            Some(TextShapeKey::content_hash(hash::hash_str("hello"))),
        );
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
        range: ops::Range<usize>,
        max_width: Option<f32>,
    }

    let m = TextShaper::new();
    let cases = [
        Case {
            label: "single_line",
            text: "hello",
            range: 1..4,
            max_width: None,
        },
        Case {
            label: "hard_breaks",
            text: "abc\ndef\nghi",
            range: 0..11,
            max_width: None,
        },
        // "def" only: lines before and after the range emit nothing.
        Case {
            label: "middle_line_only",
            text: "abc\ndef\nghi",
            range: 4..7,
            max_width: None,
        },
        // "ef\ng" — spans lines 1–2, line 0 must emit nothing.
        Case {
            label: "tail_span",
            text: "abc\ndef\nghi",
            range: 5..9,
            max_width: None,
        },
        Case {
            label: "mixed_bidi",
            text: "abc אבג def",
            range: 2..12,
            max_width: None,
        },
        Case {
            label: "soft_wrap_and_graphemes",
            text: "á one two three four five",
            range: 0..27,
            max_width: Some(48.0),
        },
    ];
    for case in cases {
        let params = match case.max_width {
            Some(w) => ui_shape(16.0).width(w),
            None => ui_shape(16.0),
        };
        let mut expected = Vec::new();
        m.probe_layout(case.text, params, |layout| {
            let buffer = layout.buffer_for_test().unwrap();
            let start = probe::cursor_from_byte(buffer, case.text, case.range.start);
            let end = probe::cursor_from_byte(buffer, case.text, case.range.end);
            for run in buffer.layout_runs() {
                // Raw `highlight` marks a run whose line differs from both
                // cursors as fully selected; cosmic's editor guards with this
                // line-range check, so the oracle must too.
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
        // Independent of the oracle: hand-computed placement for partial
        // ranges. A line is 16 × 1.2 = 19.2 px, snapped to
        // round(19.2 × 64) / 64 = 19.203125; the lines sit at y = 0, lh, 2 · lh.
        let lh = 19.203125;
        assert_eq!(16.0 * LINE_HEIGHT_MULT, 19.2, "premise: production leading");
        let ys: Vec<f32> = actual.iter().map(|r| r.min.y).collect();
        let want: &[f32] = match case.label {
            "single_line" => &[0.0],
            "middle_line_only" => &[lh],
            "tail_span" => &[lh, 2.0 * lh],
            _ => continue,
        };
        assert_eq!(ys, want, "case: {}", case.label);
        for rect in &actual {
            assert_eq!(rect.size.h, lh, "case: {}: one line tall", case.label);
        }
    }
}

/// Byte offsets map to cosmic cursors through the shaped buffer's own lines,
/// so every ending cosmic splits at (`\n`, `\r`, `\r\n`, `\n\r`) starts a
/// line there. Rows are `(offset, line, index, back)`: the cursor an offset
/// maps to and the offset it maps back to. An offset inside a two-byte ending
/// sits at its line's end; one past the text clamps to the end.
#[test]
fn byte_offsets_map_through_cosmic_lines() {
    // (offset, line, index, back)
    type Row = (usize, usize, usize, usize);

    let m = TextShaper::new();
    let rows: &[(&str, &[Row])] = &[
        (
            "ab\ncde\nfg",
            &[
                (0, 0, 0, 0),
                (2, 0, 2, 2),
                (3, 1, 0, 3),
                (6, 1, 3, 6),
                (7, 2, 0, 7),
                (9, 2, 2, 9),
            ],
        ),
        ("ab\rcd", &[(2, 0, 2, 2), (3, 1, 0, 3), (5, 1, 2, 5)]),
        (
            "ab\r\ncd",
            &[(2, 0, 2, 2), (3, 0, 2, 2), (4, 1, 0, 4), (6, 1, 2, 6)],
        ),
        (
            "ab\n\rcd",
            &[(2, 0, 2, 2), (3, 0, 2, 2), (4, 1, 0, 4), (6, 1, 2, 6)],
        ),
        (
            "ab\ncd",
            &[(6, 1, 2, 5), (99, 1, 2, 5), (usize::MAX, 1, 2, 5)],
        ),
    ];
    for &(text, offsets) in rows {
        m.probe_layout(text, ui_shape(16.0), |layout| {
            let buffer = layout.buffer_for_test().unwrap();
            for &(offset, line, index, back) in offsets {
                let cursor = probe::cursor_from_byte(buffer, text, offset);
                assert_eq!(
                    (cursor.line, cursor.index),
                    (line, index),
                    "{text:?} at {offset}"
                );
                assert_eq!(
                    probe::cursor_to_byte(buffer, text, cursor),
                    back,
                    "{text:?} at {offset}"
                );
            }
        });
    }
}

/// A truncated run shapes its kept prefix and the ellipsis, so a hit
/// answers a source byte no later than the cut, on a char boundary:
/// `"ééééé"` cut to `"é…"` answers 0 or 2, never 3 (inside the second `é`).
#[test]
fn a_truncated_run_hits_inside_its_kept_prefix() {
    let m = TextShaper::new();
    let text = "ééééé";
    let font = ui_shape(16.0).font;
    let width = m
        .layout(&TextRun {
            text: "é…",
            font,
            wrap: TextWrap::SingleLine,
            align: Align::LEFT,
            max_width: None,
        })
        .size()
        .w;
    let probe = m.layout(&TextRun {
        text,
        font,
        wrap: TextWrap::Ellipsis,
        align: Align::LEFT,
        max_width: Some(width + 0.5),
    });
    assert_eq!(probe.size().w, width, "premise: the run is cut to \"é…\"");
    // The kept `é` advances 9.328125: x left of its midpoint 4.6640625
    // answers byte 0, the rest (ellipsis included) answers the cut at byte 2.
    assert_eq!(probe.caret_at(2).x, 9.328125);
    let mut x = -2.0;
    while x < width + 4.0 {
        let want = if x < 4.6640625 { 0 } else { 2 };
        assert_eq!(probe.byte_at(x, 5.0), want, "x {x}");
        x += 0.5;
    }
    assert_eq!(
        probe.caret_at(8).x,
        probe.caret_at(2).x,
        "past the cut sits at the cut"
    );
}

/// Two-line buffer: the line-1 caret sits one line (16 × 1.2 = 19.2 px,
/// snapped to round(19.2 × 64) / 64) below line 0, pinning multi-line routing
/// through cosmic's layout_runs.
#[test]
fn cursor_xy_multiline_y_top_advances_per_line() {
    let m = TextShaper::new();
    let lh = 19.203125;
    for (byte, y_top) in [(0, 0.0), (4, lh)] {
        assert_eq!(
            m.cursor_xy("abc\ndef", byte, ui_shape(16.0)),
            Caret {
                x: 0.0,
                y_top,
                line_height: lh,
            },
            "byte {byte}",
        );
    }
}

/// Right-aligned multi-line buffer: the caret at byte 4 ("abc\n|") lands on
/// the empty second line. Cosmic reports `x = 0` there whatever the
/// alignment, so the empty-line branch uses `empty_line_x`. That edge is the
/// block's, not the wrap target's: measuring against the wrap target would
/// align twice.
#[test]
fn cursor_xy_on_empty_line_respects_right_align() {
    let m = TextShaper::new();
    let text = "abc\n";
    let shape = ui_shape(16.0).width(200.0).halign(HAlign::Right);
    let block = m.measure(text, shape).extent.size.w;
    // "abc" is 28 px, far narrower than the 200 px wrap target.
    assert_eq!(block, 28.0);
    let lh = 19.203125;
    assert_eq!(
        m.cursor_xy(text, text.len(), shape),
        Caret {
            x: block,
            y_top: lh,
            line_height: lh,
        },
        "right-aligned caret on the empty trailing line sits at the block's \
         right edge",
    );
    // Left-aligned still anchors at zero.
    assert_eq!(
        m.cursor_xy(text, text.len(), shape.halign(HAlign::Left)).x,
        0.0,
    );
}

/// A width-bounded run measures its glyphs, not the distance from the wrap
/// target's left edge. Cosmic anchors a line where alignment and direction
/// put it, so the gap before a non-left-aligned run once counted as width
/// (200 px for 43 px of glyphs), inflating a hugging container to the whole
/// offer. The RTL row is reachable without `text_align`: cosmic lays RTL runs
/// out leftward from `line_width` (`shape.rs`'s `start_x`), so `HAlign::Auto`
/// hit it too.
#[test]
fn a_bounded_run_measures_its_glyphs_not_the_gap_before_them() {
    // The RTL row is Arabic, which no bundled face covers; load the Arabic test face.
    let mut m = CosmicMeasure::new(FontScope::Bundled);
    m.load_font(ARABIC.into())
        .expect("the Arabic test face loads");
    let wrap = 200.0;
    let bounded = |halign| ui_shape(16.0).width(wrap).halign(halign);
    for (label, text) in [("LTR", "ab cd"), ("RTL", "مرحبا بالعالم")] {
        let unbounded = m.measure(text, ui_shape(16.0)).size.w;
        // The run fits on one line, so binding a width changes only where
        // cosmic puts the glyphs: every alignment reports the natural width.
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

/// Caret and hit-test must stay exact inverses: `cursor_xy` subtracts the
/// block origin and `byte_at_xy` adds it back, and a sign slip in either
/// would break the round trip in our coordinates, not cosmic's.
#[test]
fn caret_and_hit_test_round_trip_in_block_local_space() {
    let m = TextShaper::new();
    // Two hard-broken lines of different widths under right align: the narrow
    // line has a non-zero block-local offset, where an unpaired correction shows.
    let text = "wwwwww\ni";
    let shape = ui_shape(16.0).width(300.0).halign(HAlign::Right);
    for byte in [0usize, 1, 3, 6, 7, 8] {
        assert!(
            text.is_char_boundary(byte),
            "byte {byte} must be a boundary"
        );
        let caret = m.cursor_xy(text, byte, shape);
        // Probe just inside the caret so the hit lands on its glyph.
        let hit = m.byte_at_xy(
            text,
            caret.x + 0.5,
            caret.y_top + caret.line_height * 0.5,
            shape,
        );
        assert_eq!(hit, byte, "byte {byte} at x = {} round-trips", caret.x);
    }
}

/// A run's ink outsets, cross-checked against the rasterizer: glyphs drawn as
/// the backend draws them, at scale 1 from the block origin. Coverage past
/// the block on each side must be inside the outset, and the outset no more
/// than one whole pixel past it (the rounding up).
///
/// Cases: an italic `f` hangs top right and tail left, at a variable face's
/// regular and bold; a synthetic italic leans right via skew alone; a ring
/// above a capital and a descender below leave the line box; a negative left
/// bearing starts left of the pen. A plain `x` stays inside on every side.
#[test]
fn ink_outsets_cover_what_the_rasterizer_draws() {
    let inter_regular_only = || {
        let mut m = CosmicMeasure::with_no_fonts();
        let family = m
            .load_font(FontSource::from(INTER))
            .expect("the bundled Inter loads");
        (m, family)
    };
    // (label, text, slant, weight, leading = size, sides that must reach)
    let cases = [
        (
            "italic",
            "f",
            FontSlant::Italic,
            FontWeight::REGULAR,
            false,
            [true, false, true, false],
        ),
        (
            "bold italic",
            "f",
            FontSlant::Italic,
            FontWeight::BOLD,
            false,
            [true, false, true, false],
        ),
        (
            "ring and descender",
            "Åg",
            FontSlant::Normal,
            FontWeight::REGULAR,
            true,
            [false, true, false, true],
        ),
        (
            "negative bearing",
            "j",
            FontSlant::Normal,
            FontWeight::REGULAR,
            false,
            [true, false, false, false],
        ),
        (
            "inside",
            "x",
            FontSlant::Normal,
            FontWeight::REGULAR,
            false,
            [false, false, false, false],
        ),
    ];
    let synthetic = (
        "synthetic italic",
        "f",
        FontSlant::Italic,
        FontWeight::REGULAR,
        false,
        [false, false, true, false],
    );
    for (index, (label, text, slant, weight, tight, reaches)) in
        cases.into_iter().chain([synthetic]).enumerate()
    {
        let (mut m, family) = if index == cases.len() {
            inter_regular_only()
        } else {
            (CosmicMeasure::default(), FontFamily::SANS)
        };
        let base = if tight { shape(64.0) } else { ui_shape(64.0) };
        let request = base
            .family(family)
            .slant(slant)
            .weight(weight)
            .unbounded_request(text);
        let root = m.root(request, WrapFloor::Skip);

        let mut placed = Vec::new();
        m.extract_glyphs(
            request,
            RunPlacement {
                origin: glam::Vec2::ZERO,
                scale: 1.0,
                bounds: None,
            },
            &mut placed,
        );
        let (mut lo, mut hi) = (glam::IVec2::MAX, glam::IVec2::MIN);
        for glyph in &placed {
            let Some(image) = m.rasterize_glyph(glyph.raster_key) else {
                continue;
            };
            let top_left = glam::IVec2::new(glyph.x + image.bearing.x, glyph.y - image.bearing.y);
            lo = lo.min(top_left);
            hi = hi.max(top_left + image.size.as_ivec2());
        }
        let past = |reach: i32| reach.max(0) as f32;
        let drawn = [
            past(-lo.x),
            past(-lo.y),
            past(hi.x - root.extent.size.w as i32),
            past(hi.y - root.extent.size.h as i32),
        ];
        let ink = root.extent.ink.as_array();
        for side in 0..4 {
            assert!(
                drawn[side] <= ink[side] && ink[side] <= drawn[side] + 1.0,
                "{label}: side {side} draws {drawn:?} past the block, ink says {ink:?}",
            );
            assert_eq!(
                ink[side] > 0.0,
                reaches[side],
                "{label}: side {side}, ink {ink:?}"
            );
        }
    }
}
