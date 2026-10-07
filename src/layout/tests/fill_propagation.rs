//! Pin: child-positioner layouts (ZStack, Canvas) and Hug-axis propagation must not pass `INFINITY` when the
//! parent has a finite slot, or a nested grid falls back to max-content and breaks wrapping.
use crate::layout::tests::support::PARAGRAPH;
use crate::primitives::identity::widget_id::WidgetId;
use crate::text::wrap::TextWrap;

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::layout::tests::support;
use crate::layout::tests::support::two_hug_cols_with_wrap;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, grid::Grid, panel::Panel, text::Text};
use glam::UVec2;

/// The paragraph wrapped at a 200 px surface: four 16 px lines in 93 px; max-content would shape one line.
fn assert_wrapped_at_200(ui: &Ui, node: NodeId) {
    assert_eq!(
        support::shaped_text(ui.layout(Layer::Main), node)
            .extent
            .size,
        Size::new(93.0, support::lines_h(4, 16.0)),
    );
}

/// A constrained ZStack must pass its inner size to children, not `INFINITY`, or a nested grid falls back to max-content.
#[test]
fn fill_zstack_passes_finite_avail_so_nested_grid_constrains() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let mut node = None;
    h.frame(|ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                node = Some(two_hug_cols_with_wrap(ui, PARAGRAPH));
            });
    });
    assert_wrapped_at_200(&h.ui, node.unwrap());
}

/// As above for Canvas, also a child-positioner layout.
#[test]
fn fill_canvas_passes_finite_avail_so_nested_grid_constrains() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let mut node = None;
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                node = Some(two_hug_cols_with_wrap(ui, PARAGRAPH));
            });
    });
    assert_wrapped_at_200(&h.ui, node.unwrap());
}

/// A `Hug` ZStack containing a `Fill` child must not recursively size to it; Hug axes keep `INFINITY`.
#[test]
fn hug_zstack_does_not_recursively_size_to_fill_child() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("hug-z"))
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("fill-child"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                        .show(ui);
                    Block::new()
                        .id(WidgetId::from_hash("fixed-child"))
                        .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                        .show(ui);
                });
        });
    });
    let r = h.arranged(WidgetId::from_hash("hug-z"));
    assert_eq!(r.size.w, 60.0);
    assert_eq!(r.size.h, 40.0);
}

/// A `Hug` grid's Fill column collapses to 0 at arrange; its cells get `INFINITY` width so text stays single-line
/// and row heights don't grow on horizontal resize.
#[test]
fn hug_grid_fill_col_does_not_grow_row_height_on_horizontal_resize() {
    fn measure(surface_w: u32) -> f32 {
        let mut h = UiHarness::with_text(UVec2::new(surface_w, 400));
        let mut value_node = None;
        h.frame(|ui| {
            Grid::new()
                .auto_id()
                .cols([Track::HUG, Track::FILL])
                .rows([Track::HUG])
                .show(ui, |ui| {
                    Text::new("Label:")
                        .auto_id()
                        .font_size(14.0)
                        .grid_cell((0, 0))
                        .show(ui);
                    value_node = Some(
                        Text::new(PARAGRAPH)
                            .auto_id()
                            .font_size(14.0)
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .grid_cell((0, 1))
                            .show(ui)
                            .node(),
                    );
                });
        });
        support::shaped_text(h.ui.layout(Layer::Main), value_node.unwrap())
            .extent
            .size
            .h
    }

    let one_line = support::lines_h(1, 14.0);
    assert_eq!([measure(2000), measure(200)], [one_line, one_line]);
}

/// A `Fill` grid with a `Fill` column wraps text in it: measure and arrange agree on its width.
#[test]
fn fill_grid_fill_col_wraps_text_under_constrained_width() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let mut value_node = None;
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Grid::new()
                .auto_id()
                .size((Sizing::FILL, Sizing::HUG))
                .cols([Track::HUG, Track::FILL])
                .rows([Track::HUG])
                .show(ui, |ui| {
                    Text::new("Label:")
                        .auto_id()
                        .font_size(14.0)
                        .grid_cell((0, 0))
                        .show(ui);
                    value_node = Some(
                        Text::new(PARAGRAPH)
                            .auto_id()
                            .font_size(14.0)
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .grid_cell((0, 1))
                            .show(ui)
                            .node(),
                    );
                });
        });
    });
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), value_node.unwrap());
    assert!(
        shaped.extent.size.h > 32.0,
        "Fill grid + Fill col should wrap text under constrained width; got h={}",
        shaped.extent.size.h,
    );
    assert!(
        shaped.extent.size.w <= 200.0,
        "wrapped text width should fit inside surface; got w={}",
        shaped.extent.size.w,
    );
}

/// A VStack section containing a `(Fill, Hug)` Grid with a wrapping Fill column sizes to the wrapped row heights.
#[test]
fn vstack_section_with_hug_grid_and_fill_col_wrap_does_not_collapse() {
    let mut h = UiHarness::with_text(UVec2::new(400, 600));
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Grid::new()
                    .id(WidgetId::from_hash("pg"))
                    .size((Sizing::FILL, Sizing::HUG))
                    .cols([Track::HUG, Track::FILL])
                    .rows([Track::HUG, Track::HUG])
                    .show(ui, |ui| {
                        Text::new("Title:")
                            .auto_id()
                            .font_size(14.0)
                            .grid_cell((0, 0))
                            .show(ui);
                        Text::new(
                            "the quick brown fox jumps over the lazy dog \
                                 pack my box with five dozen liquor jugs how \
                                 vexingly quick daft zebras jump",
                        )
                        .auto_id()
                        .font_size(14.0)
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .grid_cell((0, 1))
                        .show(ui);
                        Text::new("Tags:")
                            .auto_id()
                            .font_size(14.0)
                            .grid_cell((1, 0))
                            .show(ui);
                        Text::new("layout, grid, intrinsic, wrapping, css")
                            .auto_id()
                            .font_size(14.0)
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .grid_cell((1, 1))
                            .show(ui);
                    });
            });
    });
    let h = h.arranged(WidgetId::from_hash("pg")).size.h;
    assert!(
        h > 50.0,
        "grid must size to wrapped row heights, not single-line × 2; got h={h}"
    );
}

/// A Hug-axis ZStack containing a Hug Grid with wrapping Fill-column cells lets the grid measure under the constrained cross axis.
#[test]
fn hug_zstack_with_nested_grid_wrap_does_not_collapse() {
    let mut h = UiHarness::with_text(UVec2::new(400, 600));
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(400.0), Sizing::HUG))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("hug-z"))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Grid::new()
                            .id(WidgetId::from_hash("nested-grid"))
                            .size((Sizing::FILL, Sizing::HUG))
                            .cols([Track::HUG, Track::FILL])
                            .rows([Track::HUG])
                            .show(ui, |ui| {
                                Text::new("Label:")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 0))
                                    .show(ui);
                                Text::new(
                                    "the quick brown fox jumps over the lazy dog \
                                         pack my box with five dozen liquor jugs",
                                )
                                .auto_id()
                                .font_size(14.0)
                                .text_wrap(TextWrap::WrapWithOverflow)
                                .grid_cell((0, 1))
                                .show(ui);
                            });
                    });
            });
    });
    let h = h.arranged(WidgetId::from_hash("nested-grid")).size.h;
    assert!(
        h > 30.0,
        "ZStack must pass `INF` on Hug axes so nested grid measures \
         under the constrained cross and wraps; got h={h}"
    );
}
