//! A wrapping leaf's height, and the intrinsics it reports.

use crate::internals::layout_rig::{GridTracks, LayoutRig, NodeRecorder, TextRun};
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::tests::support::{PARAGRAPH, two_hug_cols_with_wrap};
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::text::glyph_font::GlyphFont;
use crate::text::request::internals::TestShape;
use crate::text::system::{TextRunSlot, TextSystem};
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// The paragraph at 16 px in a 60 px column: 7 chars a line, so each word takes a line of its
/// own, nine lines of 16, as wide as the widest word, 5 × 8 = 40.
#[test]
fn wrapping_text_grows_height_in_narrow_frame() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 400.0), |rec| {
        let column = Widget::vstack().size((Sizing::fixed(60.0), Sizing::HUG));
        rec.node("column", column, |rec| {
            let run = TextRun::new(PARAGRAPH, 16.0, TextWrap::WrapWithOverflow);
            rec.text_leaf("text", Widget::leaf(), run);
        });
    });
    let r = rig.rect("text");
    assert_eq!(r.size, Size::new(40.0, 9.0 * 16.0), "nine lines in 60 px");
    assert_eq!(rig.shaped_text("text").extent.size, r.size);
}

/// A wrapping text in a `Grid` `Hug` column reshapes to the column's committed width, resolved in
/// measure against `inner_avail`. At 16 px the paragraph runs 40 (`quick`) to 344 wide and the
/// label 96 either way, so of 200 the paragraph's column takes `40 + (200 − 136) = 104`, 13
/// chars: "the quick", "brown fox", "jumps over", "the lazy dog", 96 wide.
#[test]
fn wrapping_text_in_grid_auto_column_wraps_under_constrained_width() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 400.0), |rec| {
        two_hug_cols_with_wrap(rec, PARAGRAPH);
    });
    assert_eq!(
        rig.shaped_text("wrapping").extent.size,
        Size::new(96.0, 4.0 * 16.0)
    );
}

/// A wrapping text leaf in a Grid `Hug` cell reports its intrinsics: 43 chars of 8 at most,
/// `quick` at least, one 16 px line tall.
#[test]
fn intrinsic_query_on_wrapping_text_leaf_returns_sensible_values() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 400.0), |rec| {
        two_hug_cols_with_wrap(rec, PARAGRAPH);
    });
    let max_w = rig.intrinsic("wrapping", Axis::X, LenReq::MaxContent);
    let min_w = rig.intrinsic("wrapping", Axis::X, LenReq::MinContent);
    let max_h = rig.intrinsic("wrapping", Axis::Y, LenReq::MaxContent);
    assert_eq!([max_w, min_w, max_h], [344.0, 40.0, 16.0]);
}

/// Pin (contains-content rule, cross axis): a FILL chrome panel around a paragraph in a Fixed(width) container grows on Y to contain the wrapped content even when surface_h is smaller. The intrinsic-min query underestimates (wrapping text runs at INF width), so the floor comes from the measure, which shaped at the laid-out width: 360 less 2 × 8 padding is 344, 49 chars of 7, three lines.
#[test]
fn fill_panel_grows_to_contain_wrapped_content_on_y() {
    const TEXT: &str = "The quick brown fox jumps over the lazy dog. \
                        Pack my box with five dozen liquor jugs. \
                        How vexingly quick daft zebras jump!";
    #[derive(Clone, Copy, Debug)]
    enum Inner {
        Stack(fn() -> Widget),
        Grid,
    }
    let inners = [
        ("vstack", Inner::Stack(Widget::vstack)),
        ("hstack", Inner::Stack(Widget::hstack)),
        ("wrap_vstack", Inner::Stack(Widget::wrap_vstack)),
        ("zstack", Inner::Stack(Widget::zstack)),
        ("grid", Inner::Grid),
    ];
    for (label, inner_kind) in inners {
        for surface_h in [800.0, 400.0, 300.0, 200.0, 150.0, 100.0, 50.0] {
            let mut rig = LayoutRig::new();
            rig.run(Size::new(800.0, surface_h), |rec| {
                let chrome = Widget::zstack()
                    .padding(16.0)
                    .size((Sizing::FILL, Sizing::FILL));
                rec.node("chrome", chrome, |rec| {
                    let size = (Sizing::fixed(360.0), Sizing::HUG);
                    let paragraph = |rec: &mut NodeRecorder<'_>| {
                        let run = TextRun::new(TEXT, 14.0, TextWrap::WrapWithOverflow);
                        rec.text_leaf("text", Widget::leaf(), run);
                    };
                    match inner_kind {
                        Inner::Stack(stack) => {
                            rec.node("inner", stack().size(size).padding(8.0), paragraph);
                        }
                        Inner::Grid => {
                            let tracks = GridTracks {
                                rows: &[Track::HUG],
                                cols: &[Track::FILL],
                            };
                            let grid = Widget::grid().size(size).padding(8.0);
                            rec.grid("inner", grid, tracks, paragraph);
                        }
                    }
                });
            });
            let height = |id| rig.rect(id).size.h;
            // Three 14 px lines plus 2 × 8 padding at every surface height; the chrome grows past the surface to its inner container plus 2 × 16 padding.
            assert_eq!(height("text"), 3.0 * 14.0, "{label} surface_h={surface_h}");
            assert_eq!(
                height("inner"),
                3.0 * 14.0 + 16.0,
                "{label} surface_h={surface_h}"
            );
            assert_eq!(
                height("chrome"),
                surface_h.max(height("inner") + 32.0),
                "{label} surface_h={surface_h}"
            );
        }
    }
}

/// A Hug vertical scroll in a 300 px column wraps its text at 300: the Hug zstack around its
/// Fill viewport, as the `Scroll` widget records them, offers children the room it can grow to,
/// where offered `INFINITY` the paragraph shaped as one overlong line. Four paragraphs at 16 px,
/// 37 chars a line, make five lines, the widest 37 × 8 = 296.
#[test]
fn a_hug_scroll_wraps_its_text_at_the_column_width() {
    let paragraph = [PARAGRAPH; 4].join(" ");
    let mut rig = LayoutRig::new();
    rig.run(Size::new(600.0, 400.0), |rec| {
        let column = Widget::vstack().size((Sizing::fixed(300.0), Sizing::HUG));
        rec.node("column", column, |rec| {
            rec.node("scroll", Widget::zstack(), |rec| {
                let axes = ScrollAxes::VERTICAL.fit_content(SizeSpec::default());
                let viewport = Widget::scroll(axes).size((Sizing::FILL, Sizing::FILL));
                rec.node("viewport", viewport, |rec| {
                    let run = TextRun::new(&paragraph, 16.0, TextWrap::WrapWithOverflow);
                    rec.text_leaf("text", Widget::leaf(), run);
                });
            });
        });
    });
    assert_eq!(
        rig.shaped_text("text").extent.size,
        Size::new(296.0, 5.0 * 16.0)
    );
}

/// Truncation under the shaper that ships, not only the mono model: an ellipsized label in a
/// 120 px column measures to what cosmic cuts it to at 120, which reserves the `…` and cuts at a
/// cluster and so falls short of the column, where the mono model fills it; the leaf's rect and
/// its shaped run agree, one line tall.
#[test]
fn an_ellipsized_label_lays_out_as_cosmic_cuts_it() {
    const LABEL: &str = "Screenshot 2026-05-28 at 01.21.25.png";
    let lay_out = |mut rig: LayoutRig| {
        rig.run(Size::new(400.0, 400.0), |rec| {
            let column = Widget::vstack().size((Sizing::fixed(120.0), Sizing::HUG));
            rec.node("column", column, |rec| {
                let run = TextRun::new(LABEL, 16.0, TextWrap::Ellipsis);
                rec.text_leaf("label", Widget::leaf(), run);
            });
        });
        rig
    };
    let cosmic = lay_out(LayoutRig::cosmic());
    let cut = TextSystem::cosmic().shape_run(
        TextRunSlot {
            widget_id: WidgetId::from_hash("label"),
            ordinal: 0,
        },
        LABEL,
        TestShape::new(GlyphFont::new(16.0)).width(120.0),
        TextWrap::Ellipsis,
    );
    let rect = cosmic.rect("label");
    assert_eq!(rect.size, cut.size, "the leaf measures to cosmic's cut");
    assert_eq!(cosmic.shaped_text("label").extent.size, rect.size);
    assert!(
        rect.size.w < 120.0,
        "the cut falls short of the column: {rect:?}"
    );

    let mono = lay_out(LayoutRig::new());
    assert_eq!(
        mono.rect("label").size,
        Size::new(120.0, 16.0),
        "the mono model fills it"
    );
}
