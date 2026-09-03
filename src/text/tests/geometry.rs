use super::*;
use crate::common::hash;

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
    let m = TextShaper::test_mono();
    for (label, text, offset, fs, lh_v, expected) in cases {
        assert_eq!(
            m.cursor_xy(text, *offset, shape(*fs).leading(*lh_v)).x,
            *expected,
            "case: {label}"
        );
    }
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
    let line_right = shaper.measure(rtl_text, shape).measured.w;
    // The measured extent has to span every glyph rather than stop at the
    // last one in the array, which in an RTL run is the leftmost — that
    // would report a four-character run as one character wide and let the
    // backend clip it.
    let one_char = shaper.measure("\u{5e9}", shape).measured.w;
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
    // Real shaping: sweeping x across a run must never walk the answer
    // backwards, and an x past the end must clamp. The exact
    // caret → hit → caret identity is a stronger claim and is pinned
    // separately by `caret_and_hit_test_round_trip_in_block_local_space`;
    // this one holds even where a boundary x is ambiguous between two
    // adjacent offsets.
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
        let params = match case.max_width_px {
            Some(w) => ui_shape(16.0).width(w),
            None => ui_shape(16.0),
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
                assert!(
                    (actual[0].size.h - lh).abs() < 0.5,
                    "one line tall (16 px × {LINE_HEIGHT_MULT}), got {}",
                    actual[0].size.h,
                );
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
    let shape = ui_shape(font).width(wrap).halign(HAlign::Right);
    let block = m.measure(text, shape).measured.w;
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
    let pos_left = m.cursor_xy(text, text.len(), shape.halign(HAlign::Left));
    assert!(
        pos_left.x.abs() < 0.5,
        "left-aligned caret on empty trailing line stays at 0; \
         got x = {}",
        pos_left.x,
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
    // The RTL row is Arabic, which no bundled face covers, so this case
    // needs the machine's fonts to shape anything but tofu — the one
    // reason a text case asks for [`FontScope::System`].
    let mut m = CosmicMeasure::new(FontScope::System);
    let wrap = 200.0;
    let bounded = |halign| ui_shape(16.0).width(wrap).halign(halign);
    for (label, text) in [("LTR", "ab cd"), ("RTL", "مرحبا بالعالم")] {
        let unbounded = m.measure(text, ui_shape(16.0)).size.w;
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
    let shape = ui_shape(16.0).width(300.0).halign(HAlign::Right);
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
