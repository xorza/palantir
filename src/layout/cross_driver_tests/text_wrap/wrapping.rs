//! A wrapping leaf's height, its truncating peer, and the intrinsics both
//! report.

use crate::TextStyle;
use crate::Ui;
use crate::layout::axis::Axis;
use crate::layout::cross_driver_tests::support;
use crate::layout::cross_driver_tests::support::two_hug_cols_with_wrap;
use crate::layout::cross_driver_tests::text_wrap::support::PARAGRAPH;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::scene::shapes::record::ShapeRecord;
use crate::text::wrap::TextWrap;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{button::Button, panel::Panel, text::Text};
use glam::UVec2;

/// The measured height of `lines` lines of `font_px` text: the default
/// style's line height per line, ceiled to whole pixels as the measurer
/// does. Widths have no such formula — they are the bundled faces' glyph
/// advances, the same on every machine.
fn lines_h(lines: u32, font_px: f32) -> f32 {
    (lines as f32 * TextStyle::default().line_height_for(font_px)).ceil()
}

#[test]
fn wrapping_text_grows_height_in_narrow_frame() {
    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    let mut text_node = None;
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(60.0), Sizing::HUG))
            .show(ui, |ui| {
                text_node = Some(
                    Text::new(PARAGRAPH)
                        .auto_id()
                        .font_size(16.0)
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui)
                        .node(),
                );
            });
    });
    let node = text_node.unwrap();
    let r = h.ui.arranged_rect(Layer::Main, node);
    assert_eq!(
        r.size,
        Size::new(60.0, lines_h(8, 16.0)),
        "eight lines in 60 px"
    );

    let shape =
        h.ui.tree(Layer::Main)
            .shapes_of(node)
            .next()
            .expect("text shape");
    let wrap = match shape {
        ShapeRecord::Text { wrap, .. } => *wrap,
        _ => panic!("expected ShapeRecord::Text"),
    };
    assert_eq!(wrap, TextWrap::WrapWithOverflow);
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), node);
    assert_eq!(shaped.measured, r.size);
}

/// A `Button` with a label wider than its `Fixed` width elides to one
/// line instead of overflowing or wrapping *by default*: the body height
/// stays a single line (contrast
/// `wrapping_text_grows_height_in_narrow_frame`,
/// where the same paragraph spans many) and the label shape carries
/// `TextWrap::SingleLine`.
#[test]
fn button_label_truncates_one_line_in_narrow_frame_by_default() {
    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    let mut node = None;
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(80.0), Sizing::HUG))
            .show(ui, |ui| {
                node = Some(Button::new().auto_id().label(PARAGRAPH).show(ui).node());
            });
    });
    let node = node.unwrap();

    let wrap =
        h.ui.tree(Layer::Main)
            .shapes_of(node)
            .find_map(|s| match s {
                ShapeRecord::Text { wrap, .. } => Some(*wrap),
                _ => None,
            })
            .expect("button label text shape");
    assert_eq!(
        wrap,
        TextWrap::Truncate,
        "a button label defaults to the truncating wrap mode"
    );

    // Elided, the paragraph stays one line, cut to fit the 80 px box
    // less the button's 2 × 12 padding and 2 × 1 border: 54 px of room.
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), node);
    assert_eq!(shaped.measured, Size::new(52.0, lines_h(1, 16.0)));
}

/// A wrapping `Text` inside a
/// `Grid` `Hug` column constrained by the parent's available width
/// reshapes to fit. The grid column-resolution algorithm runs during
/// measure with the grid's `inner_avail` (200 px here); the wrapping
/// text gets its committed column width before shaping, so the cached
/// shape is multi-line and fits the slot.
#[test]
fn wrapping_text_in_grid_auto_column_wraps_under_constrained_width() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let node = h.frame_value(|ui| two_hug_cols_with_wrap(ui, PARAGRAPH));
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), node);
    // Four lines at the resolved column width, inside the 200 px surface.
    assert_eq!(shaped.measured, Size::new(93.0, lines_h(4, 16.0)));
}

/// `Ui::intrinsic` returns sane values for a wrapping text leaf
/// inside a Grid `Auto` cell. Pure infrastructure test — confirms
/// the API + cache + per-driver functions are wired correctly.
#[test]
fn intrinsic_query_on_wrapping_text_leaf_returns_sensible_values() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let node = h.frame_value(|ui| two_hug_cols_with_wrap(ui, PARAGRAPH));
    let max_w = h.intrinsic(node, Axis::X, LenReq::MaxContent);
    let min_w = h.intrinsic(node, Axis::X, LenReq::MinContent);
    let max_h = h.intrinsic(node, Axis::Y, LenReq::MaxContent);

    // The unbroken paragraph, its widest word, and one line.
    assert_eq!([max_w, min_w, max_h], [335.0, 48.0, lines_h(1, 16.0)]);
}

/// Pin (contains-content rule, cross axis): a FILL chrome panel
/// wrapping a paragraph in a Fixed(width) inner panel must grow on Y
/// to contain its wrapped content, even when surface_h is smaller.
/// The intrinsic-min query alone underestimates this (wrapping text
/// intrinsic runs at INF width → single-line height), so the floor
/// has to come from the post-dispatch measured content. Without the
/// fix, surface_h < natural content height makes the chrome panel
/// rect shorter than its content, visibly clipping at the bottom.
#[test]
fn fill_panel_grows_to_contain_wrapped_content_on_y() {
    use crate::scene::tree::node_id::NodeId;
    use crate::widgets::panel::Panel;
    fn build(ui: &mut Ui) -> (NodeId, NodeId) {
        let mut inner = NodeId(0);
        Panel::zstack()
            .auto_id()
            .padding(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                inner = Panel::vstack()
                    .id_salt("inner")
                    .size((Sizing::fixed(360.0), Sizing::HUG))
                    .padding(8.0)
                    .show(ui, |ui| {
                        Text::new(
                            "The quick brown fox jumps over the lazy dog. \
                             Pack my box with five dozen liquor jugs. \
                             How vexingly quick daft zebras jump!",
                        )
                        .auto_id()
                        .font_size(14.0)
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui);
                    })
                    .response
                    .node();
            });
        // The chrome panel is the first child of the implicit root.
        (NodeId(1), inner)
    }
    // The inner Fixed-width panel is Hug on Y, so its rect.size.h is the
    // measured wrapped-paragraph height (+ inner padding). Chrome must
    // be at least that + chrome padding (16*2 = 32) on Y, at every
    // surface height — including ones smaller than the natural content.
    for h in [800u32, 400, 300, 200, 150, 100, 50] {
        let mut harness = UiHarness::with_text(UVec2::new(800, h));
        let mut nodes = (NodeId(0), NodeId(0));
        harness.frame(|ui| {
            nodes = build(ui);
        });
        let (chrome, inner) = nodes;
        let chrome_h = harness.ui.arranged_rect(Layer::Main, chrome).size.h;
        let inner_h = harness.ui.arranged_rect(Layer::Main, inner).size.h;
        // Three 14 px lines and 2 × 8 padding, unless the surface is too
        // short to offer them (see ISSUES). The chrome fills the surface,
        // and grows past it to its inner panel plus 2 × 16 padding.
        let expected_inner = if h > 50 {
            lines_h(3, 14.0) + 16.0
        } else {
            lines_h(1, 14.0) + 16.0
        };
        assert_eq!(inner_h, expected_inner, "surface_h={h}");
        assert_eq!(chrome_h, (h as f32).max(inner_h + 32.0), "surface_h={h}");
    }
}

/// A Hug `Scroll::vertical()` in a 300 px column wraps its text at 300.
/// The scroll's outer frame is a Hug ZStack around a Fill viewport, and a
/// Hug ZStack offers its children the room it can grow to — the column's
/// 300 px — as `Stack` offers its cross axis. Offered `INFINITY` there,
/// the paragraph shaped as one line far wider than the column, and the
/// viewport clipped it.
#[test]
fn a_hug_scroll_wraps_its_text_at_the_column_width() {
    use crate::widgets::scroll::Scroll;

    let paragraph = [PARAGRAPH; 4].join(" ");
    let mut h = UiHarness::with_text(UVec2::new(600, 400));
    let mut text_node = None;
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(300.0), Sizing::HUG))
            .show(ui, |ui| {
                Scroll::vertical().auto_id().show(ui, |ui| {
                    text_node = Some(
                        Text::new(&paragraph)
                            .auto_id()
                            .font_size(16.0)
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .show(ui)
                            .node(),
                    );
                });
            });
    });
    let node = text_node.unwrap();
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), node);
    // Five lines inside the 300 px column. 97 rather than `lines_h(5)`'s
    // 96: cosmic's accumulated `line_top` lands the fifth line's bottom at
    // 96.00001, which ceils up (see ISSUES).
    assert_eq!(shaped.measured, Size::new(285.0, 97.0));
}
