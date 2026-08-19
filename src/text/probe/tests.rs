use crate::layout::types::align::Align;
use crate::text::glyph_font::GlyphFont;
use crate::text::run::TextRun;
use crate::text::wrap::TextWrap;
use crate::text::{FontFamily, FontWeight};
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
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
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
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
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
}
