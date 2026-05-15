//! Pin: child-positioner layouts (ZStack, Canvas) and Hug-axis
//! propagation must not silently switch to `INFINITY` when the
//! parent has a finite slot — that would make any nested grid fall
//! back to max-content and break wrapping under constrained widths.

use super::support;
use super::support::two_hug_cols_with_wrap;
use crate::TextStyle;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::forest::tree::NodeId;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::support::internals::ResponseNodeExt;
use crate::support::testing::{run_at_acked, ui_with_text};
use crate::widgets::{frame::Frame, grid::Grid, panel::Panel, text::Text};
use glam::UVec2;
use std::rc::Rc;

const PARAGRAPH: &str = "the quick brown fox jumps over the lazy dog";

fn assert_wrapped_within_surface(ui: &Ui, node: NodeId, surface_w: f32) {
    let shaped = support::shaped_text(&ui.layout[Layer::Main], node);
    assert!(
        shaped.measured.h > 32.0,
        "expected multi-line wrapped height, got h={}",
        shaped.measured.h,
    );
    assert!(
        shaped.measured.w <= surface_w,
        "wrapped text must fit inside surface ({surface_w}); got w={}",
        shaped.measured.w,
    );
}

/// Regression: a constrained ZStack (`Sizing::Fill`/`Fixed`) must pass
/// its inner size to children, not `INFINITY`. Without this,
/// Grid Auto resolution falls back to max-content for any grid nested
/// inside a ZStack (Phase-1 column intrinsics need a finite slot).
#[test]
fn fill_zstack_passes_finite_avail_so_nested_grid_constrains() {
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let mut node = None;
    run_at_acked(&mut ui, UVec2::new(200, 400), |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                node = Some(two_hug_cols_with_wrap(ui, PARAGRAPH));
            });
    });
    assert_wrapped_within_surface(&ui, node.unwrap(), 200.0);
}

/// Regression: same as above but for Canvas — also a "child-positioner"
/// layout that historically passed `INFINITY` regardless of its own size.
#[test]
fn fill_canvas_passes_finite_avail_so_nested_grid_constrains() {
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let mut node = None;
    run_at_acked(&mut ui, UVec2::new(200, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                node = Some(two_hug_cols_with_wrap(ui, PARAGRAPH));
            });
    });
    assert_wrapped_within_surface(&ui, node.unwrap(), 200.0);
}

/// Pin: a `Hug` ZStack containing a `Fill` child must NOT recursively
/// size to its child. The per-axis fix above keeps the original
/// `INFINITY` behavior on Hug axes precisely to avoid this.
#[test]
fn hug_zstack_does_not_recursively_size_to_fill_child() {
    let mut ui = Ui::default();
    let mut zstack_node = None;
    run_at_acked(&mut ui, UVec2::new(800, 600), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            zstack_node = Some(
                Panel::zstack()
                    .id_salt("hug-z")
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("fill-child")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: Color::rgb(0.5, 0.5, 0.5).into(),
                                ..Default::default()
                            })
                            .show(ui);
                        Frame::new()
                            .id_salt("fixed-child")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                            .show(ui);
                    })
                    .node(ui),
            );
        });
    });
    let r = ui.layout[Layer::Main].rect[zstack_node.unwrap().index()];
    assert_eq!(r.size.w, 60.0);
    assert_eq!(r.size.h, 40.0);
}

/// Pin: a `Hug` grid with a `Fill` column has the Fill column collapse
/// to 0 at arrange (no leftover available). The measure pass handles
/// this by leaving Fill cols unresolved → cells in Fill cols get
/// `INFINITY` available width → text shapes at natural (single line),
/// so row heights don't grow weirdly when the window resizes
/// horizontally.
#[test]
fn hug_grid_fill_col_does_not_grow_row_height_on_horizontal_resize() {
    fn measure(surface_w: u32) -> f32 {
        let mut ui = ui_with_text(UVec2::new(surface_w, 400));
        let mut value_node = None;
        run_at_acked(&mut ui, UVec2::new(surface_w, 400), |ui| {
            Grid::new()
                .auto_id()
                .cols(Rc::from([Track::hug(), Track::fill()]))
                .rows(Rc::from([Track::hug()]))
                .show(ui, |ui| {
                    Text::new("Label:")
                        .auto_id()
                        .style(TextStyle::default().with_font_size(14.0))
                        .grid_cell((0, 0))
                        .show(ui);
                    value_node = Some(
                        Text::new("the quick brown fox jumps over the lazy dog")
                            .auto_id()
                            .style(TextStyle::default().with_font_size(14.0))
                            .wrapping()
                            .grid_cell((0, 1))
                            .show(ui)
                            .node(ui),
                    );
                });
        });
        support::shaped_text(&ui.layout[Layer::Main], value_node.unwrap())
            .measured
            .h
    }

    let h_wide = measure(2000);
    let h_narrow = measure(200);
    assert!(
        h_wide < 24.0,
        "wide-window value should be single-line in Hug grid, got h={h_wide}"
    );
    assert!(
        h_narrow < 24.0,
        "narrow-window value should also be single-line (Fill col gets INF avail in Hug grid), got h={h_narrow}"
    );
    assert!(
        (h_wide - h_narrow).abs() < 0.5,
        "row height must not change with horizontal resize in Hug grid + Fill col; \
         wide={h_wide}, narrow={h_narrow}",
    );
}

/// Pin: a `Fill` grid with a `Fill` column DOES wrap text in the Fill
/// column — measure and arrange agree on the Fill col width (both equal
/// inner_avail's leftover after Hug + Fixed). This is the property-grid
/// pattern.
#[test]
fn fill_grid_fill_col_wraps_text_under_constrained_width() {
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let mut value_node = None;
    run_at_acked(&mut ui, UVec2::new(200, 400), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Grid::new()
                .auto_id()
                .size((Sizing::FILL, Sizing::Hug))
                .cols(Rc::from([Track::hug(), Track::fill()]))
                .rows(Rc::from([Track::hug()]))
                .show(ui, |ui| {
                    Text::new("Label:")
                        .auto_id()
                        .style(TextStyle::default().with_font_size(14.0))
                        .grid_cell((0, 0))
                        .show(ui);
                    value_node = Some(
                        Text::new("the quick brown fox jumps over the lazy dog")
                            .auto_id()
                            .style(TextStyle::default().with_font_size(14.0))
                            .wrapping()
                            .grid_cell((0, 1))
                            .show(ui)
                            .node(ui),
                    );
                });
        });
    });
    let shaped = support::shaped_text(&ui.layout[Layer::Main], value_node.unwrap());
    assert!(
        shaped.measured.h > 32.0,
        "Fill grid + Fill col should wrap text under constrained width; got h={}",
        shaped.measured.h,
    );
    assert!(
        shaped.measured.w <= 200.0,
        "wrapped text width should fit inside surface; got w={}",
        shaped.measured.w,
    );
}

/// Regression: a VStack section containing a `(Fill, Hug)` Grid with a
/// Hug+Fill column layout and wrapping text in the Fill col must size
/// to the *wrapped* row heights, not the single-line intrinsic.
#[test]
fn vstack_section_with_hug_grid_and_fill_col_wrap_does_not_collapse() {
    let mut ui = ui_with_text(UVec2::new(400, 600));
    let mut grid_node = None;
    run_at_acked(&mut ui, UVec2::new(400, 600), |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                grid_node = Some(
                    Grid::new()
                        .id_salt("pg")
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug(), Track::hug()]))
                        .show(ui, |ui| {
                            Text::new("Title:")
                                .auto_id()
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((0, 0))
                                .show(ui);
                            Text::new(
                                "the quick brown fox jumps over the lazy dog \
                                 pack my box with five dozen liquor jugs how \
                                 vexingly quick daft zebras jump",
                            )
                            .auto_id()
                            .style(TextStyle::default().with_font_size(14.0))
                            .wrapping()
                            .grid_cell((0, 1))
                            .show(ui);
                            Text::new("Tags:")
                                .auto_id()
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((1, 0))
                                .show(ui);
                            Text::new("layout, grid, intrinsic, wrapping, css")
                                .auto_id()
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((1, 1))
                                .show(ui);
                        })
                        .node(ui),
                );
            });
    });
    let h = ui.layout[Layer::Main].rect[grid_node.unwrap().index()]
        .size
        .h;
    assert!(
        h > 50.0,
        "grid must size to wrapped row heights, not single-line × 2; got h={h}"
    );
}

/// Regression: a Hug-axis ZStack containing a Hug Grid with wrapping
/// cells in a Fill col must let the grid measure under the constrained
/// cross axis.
#[test]
fn hug_zstack_with_nested_grid_wrap_does_not_collapse() {
    let mut ui = ui_with_text(UVec2::new(400, 600));
    let mut grid_node = None;
    run_at_acked(&mut ui, UVec2::new(400, 600), |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::Fixed(400.0), Sizing::Hug))
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("hug-z")
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        grid_node = Some(
                            Grid::new()
                                .id_salt("nested-grid")
                                .size((Sizing::FILL, Sizing::Hug))
                                .cols(Rc::from([Track::hug(), Track::fill()]))
                                .rows(Rc::from([Track::hug()]))
                                .show(ui, |ui| {
                                    Text::new("Label:")
                                        .auto_id()
                                        .style(TextStyle::default().with_font_size(14.0))
                                        .grid_cell((0, 0))
                                        .show(ui);
                                    Text::new(
                                        "the quick brown fox jumps over the lazy dog \
                                         pack my box with five dozen liquor jugs",
                                    )
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .wrapping()
                                    .grid_cell((0, 1))
                                    .show(ui);
                                })
                                .node(ui),
                        );
                    });
            });
    });
    let h = ui.layout[Layer::Main].rect[grid_node.unwrap().index()]
        .size
        .h;
    assert!(
        h > 30.0,
        "ZStack must pass `INF` on Hug axes so nested grid measures \
         under the constrained cross and wraps; got h={h}"
    );
}
