use crate::internals::harness::UiHarness;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::Align;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::run::TextRun;
use crate::text::wrap::TextWrap;

/// The public probe surface against the mono shaper's exact metric (8 px per glyph at 16 px), written as a
/// caller would, through `Ui::probe_text` alone.
#[test]
fn probing_a_run_maps_bytes_and_positions_both_ways() {
    fn run(text: &str, max_width: Option<f32>) -> TextRun<'_> {
        TextRun {
            text,
            font: GlyphFont {
                size: 16.0,
                line_height: 20.0,
                family: FontFamily::SANS,
                weight: FontWeight::REGULAR,
                slant: FontSlant::Normal,
            },
            wrap: TextWrap::SingleLine,
            align: Align::LEFT,
            max_width,
        }
    }

    const EM: f32 = 8.0; // 16 px font, mono half-width advance.
    let mut harness = UiHarness::arena();
    let ui = harness.ui();

    {
        let probe = ui.probe_text(run("hello", None));
        assert_eq!(probe.size().w, 5.0 * EM, "run width is 5 mono glyphs");

        for byte in 0..=5 {
            assert_eq!(
                probe.caret_at(byte).x,
                byte as f32 * EM,
                "caret at byte {byte}",
            );
        }

        assert_eq!(probe.byte_at(3.0, 0.0), 0, "left half of glyph 0");
        assert_eq!(probe.byte_at(5.0, 0.0), 1, "right half of glyph 0");
        assert_eq!(probe.byte_at(3.0 * EM, 0.0), 3, "exactly on a boundary");
        assert_eq!(probe.byte_at(999.0, 0.0), 5, "past the end clamps");

        let mut rects = Vec::new();
        probe.selection_rects(1..4, |rect| rects.push(rect));
        assert_eq!(rects.len(), 1, "one visual line, one rect");
        assert_eq!(rects[0].min.x, EM);
        assert_eq!(rects[0].size.w, 3.0 * EM);

        let mut none = Vec::new();
        probe.selection_rects(2..2, |rect| none.push(rect));
        assert!(none.is_empty(), "an empty selection has no rects");
    }

    // A `SingleLine` run keeps its unbounded shape when given a width (the `TextWrap::line_fit` mapping).
    let bounded = ui.probe_text(run("hello", Some(16.0))).size().w;
    assert_eq!(bounded, 5.0 * EM, "a width is inert on SingleLine");

    let a = ui.probe_text(run("hello", None)).text_hash();
    let b = ui.probe_text(run("hello", None)).text_hash();
    let c = ui.probe_text(run("hellO", None)).text_hash();
    assert_eq!(a, b, "same text, same hash");
    assert_ne!(a, c, "one changed byte changes the hash");
}

/// Wrapping runs bind their width, so the same text reflows to a greater height.
#[test]
fn a_wrapping_run_binds_its_width_and_a_single_line_run_does_not() {
    let mut harness = UiHarness::arena();
    let ui = harness.ui();
    let run = |wrap, max_width| TextRun {
        text: "hello world",
        font: GlyphFont {
            size: 16.0,
            line_height: 20.0,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        },
        wrap,
        align: Align::LEFT,
        max_width,
    };

    let single = ui.probe_text(run(TextWrap::SingleLine, Some(40.0))).size();
    assert_eq!(single.w, 88.0);

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

    // A non-finite `max_width` (a public field filled from caller arithmetic) binds nothing.
    let unbounded = ui.probe_text(run(TextWrap::Wrap, None)).size();
    for width in [f32::INFINITY, f32::NAN] {
        let got = ui.probe_text(run(TextWrap::Wrap, Some(width))).size();
        assert_eq!(
            got, unbounded,
            "a {width} width must leave the run unbounded"
        );
    }
}

/// A face the shaper cannot be asked for (caller-built `GlyphFont`) measures nothing, like empty text.
#[test]
fn an_unusable_face_probes_to_nothing() {
    let mut harness = UiHarness::arena();
    let ui = harness.ui();
    let run = |size, line_height| TextRun {
        text: "hello",
        font: GlyphFont {
            size,
            line_height,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        },
        wrap: TextWrap::SingleLine,
        align: Align::LEFT,
        max_width: None,
    };

    assert_eq!(ui.probe_text(run(16.0, 20.0)).size().w, 5.0 * 8.0);
    for (size, line_height, label) in [
        (0.0, 20.0, "zero size"),
        (f32::NAN, 20.0, "NaN size"),
        (f32::INFINITY, 20.0, "infinite size"),
        (16.0, 0.0, "zero leading"),
        (16.0, f32::NAN, "NaN leading"),
    ] {
        let probe = ui.probe_text(run(size, line_height));
        assert_eq!(probe.size(), Size::ZERO, "{label} must measure nothing");
        assert_eq!(
            probe.caret_at(3).x,
            0.0,
            "{label} must put every caret at the origin",
        );
    }
}
