//! A wrapping leaf's height, its truncating peer, and the intrinsics both
//! report.

use crate::TextStyle;
use crate::Ui;
use crate::layout::cross_driver_tests::support;
use crate::layout::cross_driver_tests::support::two_hug_cols_with_wrap;
use crate::layout::cross_driver_tests::text_wrap::support::PARAGRAPH;
use crate::layout::types::sizing::Sizing;
use crate::layout::{axis::Axis, intrinsic::LenReq};
use crate::scene::layer::Layer;
use crate::scene::shapes::record::ShapeRecord;
use crate::text::wrap::TextWrap;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{button::Button, panel::Panel, text::Text};
use glam::UVec2;

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
                        .style(&TextStyle::default().with_font_size(16.0))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui)
                        .node(),
                );
            });
    });
    let node = text_node.unwrap();
    let r = h.ui.arranged_rect(Layer::Main, node);
    assert!(
        r.size.h > 32.0,
        "wrapped paragraph should span multiple lines, got h={}",
        r.size.h,
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
    assert!(shaped.measured.h > 32.0);
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

    // The same paragraph wraps to >32 px tall in the wrap test; elided it
    // stays a single ~16 px line.
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), node);
    assert!(
        shaped.measured.h <= 32.0,
        "elided label must stay one line, got h={}",
        shaped.measured.h,
    );
    // And the elided line fits the button's fixed width (label width is
    // bounded by the 80 px box minus its padding).
    assert!(
        shaped.measured.w <= 80.0,
        "elided label must fit the button width, got w={}",
        shaped.measured.w,
    );
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
    // 16 px font wraps to 3 lines at resolved col width — h ≈ 58.
    assert!(
        shaped.measured.h > 32.0,
        "expected multi-line wrapped height, got h={}",
        shaped.measured.h,
    );
    assert!(
        shaped.measured.w <= 200.0,
        "expected text width within the 200 px surface, got w={}",
        shaped.measured.w,
    );
}

/// `Ui::intrinsic` returns sane values for a wrapping text leaf
/// inside a Grid `Auto` cell. Pure infrastructure test — confirms
/// the API + cache + per-driver functions are wired correctly.
#[test]
fn intrinsic_query_on_wrapping_text_leaf_returns_sensible_values() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let node = h.frame_value(|ui| two_hug_cols_with_wrap(ui, PARAGRAPH));
    let store = h.ui.record_store();
    let interned_text = store.interned_text();
    let max_w = h.engines.layout.intrinsic(
        h.ui.tree(Layer::Main),
        node,
        Axis::X,
        LenReq::MaxContent,
        &interned_text,
    );
    let min_w = h.engines.layout.intrinsic(
        h.ui.tree(Layer::Main),
        node,
        Axis::X,
        LenReq::MinContent,
        &interned_text,
    );
    let max_h = h.engines.layout.intrinsic(
        h.ui.tree(Layer::Main),
        node,
        Axis::Y,
        LenReq::MaxContent,
        &interned_text,
    );

    assert!(
        max_w > 200.0,
        "max_w should be the natural unbroken width, got {max_w}"
    );
    assert!(
        min_w > 0.0 && min_w < max_w,
        "min_w should be positive and < max_w, got {min_w}"
    );
    assert!(
        min_w < 100.0,
        "min_w should be a single-word width, got {min_w}"
    );
    assert!(
        max_h > 0.0 && max_h < 30.0,
        "max_h should be single-line height, got {max_h}"
    );
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
                        .style(&TextStyle::default().with_font_size(14.0))
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
        let floor = inner_h + 32.0;
        assert!(
            chrome_h + 0.5 >= floor,
            "FILL chrome panel must contain its inner panel on Y at surface_h={h}; \
             chrome_h={chrome_h} inner_h={inner_h} required_floor={floor}",
        );
    }
}
