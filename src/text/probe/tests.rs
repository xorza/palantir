use crate::layout::types::align::Align;
use crate::primitives::size::Size;
use crate::text::font_family::FontFamily;
use crate::text::font_style::FontStyle;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::probe::{cursor_from_byte, cursor_to_byte};
use crate::text::run::TextRun;
use crate::text::wrap::TextWrap;
use crate::ui::harness::UiHarness;

/// The whole public probe surface, against the mono shaper's exact
/// metric: every glyph is `font_size_px * 0.5` wide, so at 16 px a
/// character is 8 px and every expected value below is arithmetic
/// rather than a recorded observation.
///
/// This is deliberately written the way a caller's own widget would
/// have to write it — `Ui::probe_text` and nothing else. If the
/// public surface stops being enough to place a caret, hit-test a
/// click, or wash a selection, it stops compiling here.
#[test]
fn probing_a_run_maps_bytes_and_positions_both_ways() {
    const EM: f32 = 8.0; // 16 px font, mono half-width advance.
    let mut harness = UiHarness::arena();
    let ui = harness.ui();

    fn run(text: &str, max_width_px: Option<f32>) -> TextRun<'_> {
        TextRun {
            text,
            font: GlyphFont {
                size_px: 16.0,
                line_height_px: 20.0,
                family: FontFamily::SANS,
                weight: FontWeight::REGULAR,
                style: FontStyle::Normal,
            },
            wrap: TextWrap::SingleLine,
            align: Align::LEFT,
            max_width_px,
        }
    }

    {
        let probe = ui.probe_text(run("hello", None));
        // 5 glyphs × 8 px.
        assert_eq!(probe.size().w, 5.0 * EM, "run width is 5 mono glyphs");

        // Caret sits on glyph boundaries: byte n → n × 8 px.
        for byte in 0..=5 {
            assert_eq!(
                probe.caret_at(byte).x,
                byte as f32 * EM,
                "caret at byte {byte}",
            );
        }

        // …and the inverse rounds to the nearest boundary, so the
        // two halves of a glyph fall to opposite sides: 3 px into an
        // 8 px cell is still byte 0, 5 px is byte 1.
        assert_eq!(probe.byte_at(3.0, 0.0), 0, "left half of glyph 0");
        assert_eq!(probe.byte_at(5.0, 0.0), 1, "right half of glyph 0");
        assert_eq!(probe.byte_at(3.0 * EM, 0.0), 3, "exactly on a boundary");
        assert_eq!(probe.byte_at(999.0, 0.0), 5, "past the end clamps");

        // Selection 1..4 is three glyphs starting one in: x = 8,
        // w = 24, on the run's single line.
        let mut rects = Vec::new();
        probe.selection_rects(1..4, |rect| rects.push(rect));
        assert_eq!(rects.len(), 1, "one visual line, one rect");
        assert_eq!(rects[0].min.x, EM);
        assert_eq!(rects[0].size.w, 3.0 * EM);

        // An empty range washes nothing at all.
        let mut none = Vec::new();
        probe.selection_rects(2..2, |rect| none.push(rect));
        assert!(none.is_empty(), "an empty selection has no rects");
    }

    // A `SingleLine` run keeps its unbounded shape, so handing it a
    // width changes nothing. This is the `TextWrap::line_fit`
    // mapping holding: bind the width for a policy that never wraps
    // and the probe would key a *different* shaped buffer than the
    // paint, which is how a caret ends up off by a few pixels.
    let bounded = ui.probe_text(run("hello", Some(16.0))).size().w;
    assert_eq!(bounded, 5.0 * EM, "a width is inert on SingleLine");

    // The hash discriminates content, which is what a caller compares
    // frame to frame instead of retaining the string.
    let a = ui.probe_text(run("hello", None)).text_hash();
    let b = ui.probe_text(run("hello", None)).text_hash();
    let c = ui.probe_text(run("hellO", None)).text_hash();
    assert_eq!(a, b, "same text, same hash");
    assert_ne!(a, c, "one changed byte changes the hash");
}

/// Wrapping runs bind their width, so the same text at the same size
/// answers with a different height once it has to reflow — the other
/// side of the `line_fit` mapping above.
#[test]
fn a_wrapping_run_binds_its_width_and_a_single_line_run_does_not() {
    let mut harness = UiHarness::arena();
    let ui = harness.ui();
    let run = |wrap, max_width_px| TextRun {
        text: "hello world",
        font: GlyphFont {
            size_px: 16.0,
            line_height_px: 20.0,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            style: FontStyle::Normal,
        },
        wrap,
        align: Align::LEFT,
        max_width_px,
    };

    // 11 glyphs × 8 px = 88 px on one line, whatever width is offered.
    let single = ui.probe_text(run(TextWrap::SingleLine, Some(40.0))).size();
    assert_eq!(single.w, 88.0);

    // The same text told to wrap at 40 px cannot be 88 px wide.
    let wrapped = ui.probe_text(run(TextWrap::Wrap, Some(40.0))).size();
    assert!(
        wrapped.w <= 40.0,
        "a wrapping run fits its bound, got {}",
        wrapped.w,
    );
    assert!(
        wrapped.h > single.h,
        "and reflows onto more lines ({} vs {})",
        wrapped.h,
        single.h,
    );

    // A width that names no width binds nothing: the run keeps its
    // unbounded shape rather than committing to a wrap grid derived from
    // a non-finite number. `max_width_px` is a public field filled from a
    // caller's own arithmetic, so this is an input case.
    let unbounded = ui.probe_text(run(TextWrap::Wrap, None)).size();
    for width in [f32::INFINITY, f32::NAN] {
        let got = ui.probe_text(run(TextWrap::Wrap, Some(width))).size();
        assert_eq!(
            got, unbounded,
            "a {width} width must leave the run unbounded"
        );
    }
}

/// A face the shaper cannot be asked for measures nothing, the way empty
/// text does — the same answer `TextShape::is_noop` gives a recorded run.
/// `GlyphFont` is public and built by the caller, so an unusable size is
/// an input case rather than a logic error, and a probe that shaped
/// against it would quantize to a 1/64-px face.
#[test]
fn an_unusable_face_probes_to_nothing() {
    let mut harness = UiHarness::arena();
    let ui = harness.ui();
    let run = |size_px, line_height_px| TextRun {
        text: "hello",
        font: GlyphFont {
            size_px,
            line_height_px,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            style: FontStyle::Normal,
        },
        wrap: TextWrap::SingleLine,
        align: Align::LEFT,
        max_width_px: None,
    };

    assert_eq!(ui.probe_text(run(16.0, 20.0)).size().w, 5.0 * 8.0);
    for (size_px, line_height_px, label) in [
        (0.0, 20.0, "zero size"),
        (f32::NAN, 20.0, "NaN size"),
        (f32::INFINITY, 20.0, "infinite size"),
        (16.0, 0.0, "zero leading"),
        (16.0, f32::NAN, "NaN leading"),
    ] {
        let probe = ui.probe_text(run(size_px, line_height_px));
        assert_eq!(probe.size(), Size::ZERO, "{label} must measure nothing");
        assert_eq!(
            probe.caret_at(3).x,
            0.0,
            "{label} must put every caret at the origin",
        );
    }
}

/// Byte offset → cosmic cursor, hand-computed over `"ab\ncd"`: the two
/// lines start at bytes 0 and 3, so byte 4 is line 1, index 1.
///
/// The out-of-range case is the one this exists for. `caret_at` and
/// `selection_rects` are documented as clamped and take offsets from a
/// caller's own arithmetic, so the clamp has to bind before `line` and
/// `index` are derived — clamping only the prefix counts lines against a
/// shorter string and then measures `index` from the raw offset, which
/// puts the cursor past the end of the line it landed on.
#[test]
fn a_byte_offset_maps_to_its_line_and_clamps_to_the_text() {
    const TEXT: &str = "ab\ncd";
    let cases: &[(usize, usize, usize)] = &[
        (0, 0, 0),
        (2, 0, 2),
        (3, 1, 0),
        (4, 1, 1),
        (5, 1, 2),
        // Past the end answers the end, not byte 99 of line 1.
        (6, 1, 2),
        (99, 1, 2),
        (usize::MAX, 1, 2),
    ];
    for &(byte_offset, line, index) in cases {
        let cursor = cursor_from_byte(TEXT, byte_offset);
        assert_eq!(
            (cursor.line, cursor.index),
            (line, index),
            "byte {byte_offset}"
        );
        assert_eq!(
            cursor_to_byte(TEXT, cursor),
            byte_offset.min(TEXT.len()),
            "byte {byte_offset} must round-trip to its clamped self",
        );
    }
}
