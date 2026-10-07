//! A wrapping leaf's height, its truncating peer, and the intrinsics both report.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::tests::support;
use crate::layout::tests::support::PARAGRAPH;
use crate::layout::tests::support::lines_h;
use crate::layout::tests::support::two_hug_cols_with_wrap;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::shape::record::ShapeRecord;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widgets::{button::Button, panel::Panel, text::Text};
use glam::UVec2;

/// The wrap mode `node`'s first text shape records.
fn text_wrap_of(h: &UiHarness, node: NodeId) -> TextWrap {
    h.ui.tree(Layer::Main)
        .shapes_of(node)
        .find_map(|s| match s {
            ShapeRecord::Text { wrap, .. } => Some(*wrap),
            _ => None,
        })
        .expect("a text shape")
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
    assert_eq!(text_wrap_of(&h, node), TextWrap::WrapWithOverflow);
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), node);
    assert_eq!(shaped.extent.size, r.size);
}

/// A `Button` with a label wider than its `Fixed` width elides to one line (`TextWrap::SingleLine`) rather than overflow or wrap.
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

    assert_eq!(
        text_wrap_of(&h, node),
        TextWrap::Truncate,
        "a button label defaults to the truncating wrap mode"
    );

    // Cut to the 80 px box less 2 × 12 padding and 2 × 1 border: 54 px of room.
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), node);
    assert_eq!(shaped.extent.size, Size::new(52.0, lines_h(1, 16.0)));
}

/// A wrapping `Text` in a `Grid` `Hug` column reshapes to the column's committed width, resolved in measure against `inner_avail` (200 px).
#[test]
fn wrapping_text_in_grid_auto_column_wraps_under_constrained_width() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let node = h.frame_value(|ui| two_hug_cols_with_wrap(ui, PARAGRAPH));
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), node);
    assert_eq!(shaped.extent.size, Size::new(93.0, lines_h(4, 16.0)));
}

/// `Ui::intrinsic` returns sane values for a wrapping text leaf in a Grid `Auto` cell.
#[test]
fn intrinsic_query_on_wrapping_text_leaf_returns_sensible_values() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let node = h.frame_value(|ui| two_hug_cols_with_wrap(ui, PARAGRAPH));
    let max_w = h.intrinsic(node, Axis::X, LenReq::MaxContent);
    let min_w = h.intrinsic(node, Axis::X, LenReq::MinContent);
    let max_h = h.intrinsic(node, Axis::Y, LenReq::MaxContent);

    assert_eq!([max_w, min_w, max_h], [335.0, 48.0, lines_h(1, 16.0)]);
}

/// Pin (contains-content rule, cross axis): a FILL chrome panel around a paragraph in a Fixed(width) container grows on Y to contain the wrapped content even when surface_h is smaller. The intrinsic-min query underestimates (wrapping text runs at INF width), so the floor comes from the measure, which shaped at the laid-out width.
#[test]
fn fill_panel_grows_to_contain_wrapped_content_on_y() {
    use crate::primitives::layout::track::Track;
    use crate::scene::tree::node_id::NodeId;
    use crate::widgets::grid::Grid;
    use crate::widgets::panel::Panel;

    #[derive(Clone, Copy, Debug)]
    enum Inner {
        Panel(fn() -> Panel),
        Grid,
    }
    fn build(ui: &mut Ui, inner_kind: Inner) -> [NodeId; 3] {
        let mut inner = NodeId(0);
        let mut text = NodeId(0);
        Panel::zstack()
            .auto_id()
            .padding(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let paragraph = |ui: &mut Ui| {
                    Text::new(
                        "The quick brown fox jumps over the lazy dog. \
                         Pack my box with five dozen liquor jugs. \
                         How vexingly quick daft zebras jump!",
                    )
                    .auto_id()
                    .font_size(14.0)
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui)
                    .node()
                };
                let size = (Sizing::fixed(360.0), Sizing::HUG);
                inner = match inner_kind {
                    Inner::Panel(panel) => panel()
                        .id_salt("inner")
                        .size(size)
                        .padding(8.0)
                        .show(ui, |ui| text = paragraph(ui))
                        .response
                        .node(),
                    Inner::Grid => Grid::new()
                        .id_salt("inner")
                        .cols([Track::FILL])
                        .rows([Track::HUG])
                        .size(size)
                        .padding(8.0)
                        .show(ui, |ui| text = paragraph(ui))
                        .response
                        .node(),
                };
            });
        [NodeId(1), inner, text]
    }
    let inners = [
        ("vstack", Inner::Panel(Panel::vstack)),
        ("hstack", Inner::Panel(Panel::hstack)),
        ("wrap_vstack", Inner::Panel(Panel::wrap_vstack)),
        ("zstack", Inner::Panel(Panel::zstack)),
        ("grid", Inner::Grid),
    ];
    for (label, inner_kind) in inners {
        for h in [800u32, 400, 300, 200, 150, 100, 50] {
            let mut harness = UiHarness::with_text(UVec2::new(800, h));
            let [chrome, inner, text] = harness.frame_value(|ui| build(ui, inner_kind));
            let height = |node| harness.ui.arranged_rect(Layer::Main, node).size.h;
            // Three 14 px lines plus 2 × 8 padding at every surface height; the chrome grows past the surface to its inner container plus 2 × 16 padding.
            assert_eq!(height(text), lines_h(3, 14.0), "{label} surface_h={h}");
            assert_eq!(
                height(inner),
                lines_h(3, 14.0) + 16.0,
                "{label} surface_h={h}"
            );
            assert_eq!(
                height(chrome),
                (h as f32).max(height(inner) + 32.0),
                "{label} surface_h={h}"
            );
        }
    }
}

/// A Hug `Scroll::vertical()` in a 300 px column wraps its text at 300: its Hug ZStack offers children the room it can grow to; offered `INFINITY`, the paragraph shaped as one overlong line.
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
    // Five 19.203125 px lines end at 96.015625, which ceils to 97.
    assert_eq!(shaped.extent.size, Size::new(285.0, lines_h(5, 16.0)));
    assert_eq!(lines_h(5, 16.0), 97.0);
}
