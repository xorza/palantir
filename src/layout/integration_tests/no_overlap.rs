//! Showcase regressions where two cells from a Grid (or two
//! back-to-back grids inside a vstack) ended up painting on top of
//! each other. Pinned via arranged-rect order plus a render-pass
//! check on emitted `DrawText` x positions.

use crate::Ui;
use crate::element::Configure;
use crate::primitives::{color::Color, sizing::Sizing, stroke::Stroke, track::Track};
use crate::test_support::{RenderCmd, cmd_at, encode_cmds, ui_with_text};
use crate::widgets::{grid::Grid, panel::Panel, styled::Styled, text::Text};
use glam::UVec2;
use std::rc::Rc;

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

fn section(ui: &mut Ui, id: &'static str, body: &mut dyn FnMut(&mut Ui)) {
    Panel::vstack_with_id(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(6.0)
        .padding(8.0)
        .fill(Color::rgb(0.16, 0.18, 0.22))
        .stroke(Stroke {
            width: 1.0,
            color: Color::rgb(0.30, 0.34, 0.42),
        })
        .radius(4.0)
        .show(ui, |ui| {
            Text::with_id(("section-title", id), "title")
                .size_px(12.0)
                .show(ui);
            body(ui);
        });
}

#[test]
fn two_hug_columns_with_wrapping_text_do_not_overlap() {
    let mut ui = ui_with_text(UVec2::new(800, 600));
    let mut left = None;
    let mut right = None;
    Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Grid::new()
                .cols(Rc::from([Track::hug(), Track::hug()]))
                .rows(Rc::from([Track::hug()]))
                .show(ui, |ui| {
                    left = Some(
                        Text::new(
                            "The quick brown fox jumps over the lazy dog. Pack my box \
                             with five dozen liquor jugs. How vexingly quick daft zebras jump!",
                        )
                        .size_px(14.0)
                        .wrapping()
                        .grid_cell((0, 0))
                        .show(ui)
                        .node,
                    );
                    right = Some(
                        Text::new("right column")
                            .size_px(14.0)
                            .grid_cell((0, 1))
                            .show(ui)
                            .node,
                    );
                });
        });
    ui.end_frame();

    let layout = &ui.layout_engine.result;
    let lr = layout.rect(left.unwrap());
    let rr = layout.rect(right.unwrap());
    assert!(lr.size.w > 0.0, "left column must have a positive width");
    assert!(
        rr.min.x >= lr.max().x - 0.5,
        "right column must start at or past the left column's right edge: \
         left={lr:?}, right={rr:?}",
    );
}

#[test]
fn text_layouts_two_sections_back_to_back_no_overlap() {
    let mut ui = ui_with_text(UVec2::new(1500, 900));

    let mut hug_left = None;
    let mut hug_right = None;
    let mut prop_label = None;
    let mut prop_value = None;

    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            section(ui, "two-hug-columns", &mut |ui| {
                Grid::with_id("two-hug-inner")
                    .cols(Rc::from([Track::hug(), Track::hug()]))
                    .rows(Rc::from([Track::hug()]))
                    .gap_xy(0.0, 16.0)
                    .show(ui, |ui| {
                        hug_left = Some(
                            Text::new(PARAGRAPH)
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        hug_right = Some(
                            Text::new("right column")
                                .size_px(14.0)
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });

            section(ui, "property-grid", &mut |ui| {
                Grid::with_id("property-grid-inner")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                    .gap_xy(6.0, 16.0)
                    .show(ui, |ui| {
                        prop_label = Some(
                            Text::new("Title:")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        prop_value = Some(
                            Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });
        });
    ui.end_frame();

    let layout = &ui.layout_engine.result;
    let l1 = layout.rect(hug_left.unwrap());
    let r1 = layout.rect(hug_right.unwrap());
    let l2 = layout.rect(prop_label.unwrap());
    let r2 = layout.rect(prop_value.unwrap());

    assert!(l1.size.w > 0.0);
    assert!(l2.size.w > 0.0);
    assert!(
        r1.min.x >= l1.max().x - 0.5,
        "two-hug-columns: right cell must start past left cell. left={l1:?}, right={r1:?}",
    );
    assert!(
        r2.min.x >= l2.max().x - 0.5,
        "property-grid: value cell must start past label cell. label={l2:?}, value={r2:?}",
    );
}

/// Render-pass repro: build the property-grid pattern and inspect
/// the emitted `DrawText` commands directly.
#[test]
fn property_grid_emits_distinct_drawtext_x_positions() {
    let mut ui = ui_with_text(UVec2::new(1500, 900));
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Grid::with_id("property-grid-inner")
                .size((Sizing::FILL, Sizing::Hug))
                .cols(Rc::from([Track::hug(), Track::fill()]))
                .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                .gap_xy(6.0, 16.0)
                .show(ui, |ui| {
                    Text::new("Title:").size_px(14.0).grid_cell((0, 0)).show(ui);
                    Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                        .size_px(14.0)
                        .wrapping()
                        .grid_cell((0, 1))
                        .show(ui);
                    Text::new("Description:")
                        .size_px(14.0)
                        .grid_cell((1, 0))
                        .show(ui);
                });
        });
    ui.end_frame();

    let cmds = encode_cmds(&ui);
    let mut text_xs: Vec<f32> = Vec::new();
    for i in 0..cmds.kinds.len() {
        if let RenderCmd::DrawText(payload) = cmd_at(&cmds, i) {
            text_xs.push(payload.rect.min.x);
        }
    }
    assert!(
        text_xs.len() >= 2,
        "expected at least two DrawText cmds; got {text_xs:?}",
    );
    assert!(
        text_xs[0] != text_xs[1],
        "Title and Lorem texts must paint at different x; got {text_xs:?}",
    );
}

/// Diagnostic: full showcase repro. Catches the screenshot bug where
/// two distinct texts emit `DrawText` at the same (x, y).
#[test]
fn text_layouts_full_showcase_drawtext_dump() {
    let mut ui = ui_with_text(UVec2::new(1620, 980));
    Panel::vstack()
        .padding(12.0)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Panel::hstack()
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |_| {});
            Panel::zstack()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .show(ui, |ui| {
                    Panel::vstack()
                        .gap(16.0)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            section(ui, "two-hug-columns", &mut |ui| {
                                Grid::with_id("two-hug-inner")
                                    .cols(Rc::from([Track::hug(), Track::hug()]))
                                    .rows(Rc::from([Track::hug()]))
                                    .gap_xy(0.0, 16.0)
                                    .show(ui, |ui| {
                                        Text::new(PARAGRAPH)
                                            .size_px(14.0)
                                            .wrapping()
                                            .grid_cell((0, 0))
                                            .show(ui);
                                        Text::new("right column")
                                            .size_px(14.0)
                                            .grid_cell((0, 1))
                                            .show(ui);
                                    });
                            });
                            section(ui, "property-grid", &mut |ui| {
                                Grid::with_id("property-grid-inner")
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .cols(Rc::from([Track::hug(), Track::fill()]))
                                    .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                                    .gap_xy(6.0, 16.0)
                                    .show(ui, |ui| {
                                        Text::new("Title:")
                                            .size_px(14.0)
                                            .grid_cell((0, 0))
                                            .show(ui);
                                        Text::new(
                                            "Lorem Ipsum is simply dummy text of the printing industry.",
                                        )
                                        .size_px(14.0)
                                        .wrapping()
                                        .grid_cell((0, 1))
                                        .show(ui);
                                        Text::new("Description:")
                                            .size_px(14.0)
                                            .grid_cell((1, 0))
                                            .show(ui);
                                        Text::new(PARAGRAPH)
                                            .size_px(14.0)
                                            .wrapping()
                                            .grid_cell((1, 1))
                                            .show(ui);
                                        Text::new("Tags:")
                                            .size_px(14.0)
                                            .grid_cell((2, 0))
                                            .show(ui);
                                        Text::new("layout, grid, intrinsic, wrapping, css")
                                            .size_px(14.0)
                                            .wrapping()
                                            .grid_cell((2, 1))
                                            .show(ui);
                                    });
                            });
                        });
                });
        });
    ui.end_frame();

    let cmds = encode_cmds(&ui);
    let mut entries: Vec<(f32, f32, u64)> = Vec::new();
    for i in 0..cmds.kinds.len() {
        if let RenderCmd::DrawText(p) = cmd_at(&cmds, i) {
            entries.push((p.rect.min.x, p.rect.min.y, p.key.text_hash));
        }
    }
    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            let (xi, yi, hi) = entries[i];
            let (xj, yj, hj) = entries[j];
            if hi != hj && (xi - xj).abs() < 0.5 && (yi - yj).abs() < 0.5 {
                panic!(
                    "two distinct texts at same (x,y): #{i} hash={hi:#x} vs #{j} hash={hj:#x} at ({xi}, {yi})",
                );
            }
        }
    }
}

/// Showcase regression: the property-grid section overlapped its
/// label column ("Title:", "Description:", "Tags:") with its
/// wrapping value column.
#[test]
fn property_grid_hug_label_does_not_overlap_fill_value() {
    let mut ui = ui_with_text(UVec2::new(800, 600));
    let mut label = None;
    let mut value = None;
    Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Grid::new()
                .size((Sizing::FILL, Sizing::Hug))
                .cols(Rc::from([Track::hug(), Track::fill()]))
                .rows(Rc::from([Track::hug()]))
                .gap_xy(6.0, 16.0)
                .show(ui, |ui| {
                    label = Some(
                        Text::new("Title:")
                            .size_px(14.0)
                            .grid_cell((0, 0))
                            .show(ui)
                            .node,
                    );
                    value = Some(
                        Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                            .size_px(14.0)
                            .wrapping()
                            .grid_cell((0, 1))
                            .show(ui)
                            .node,
                    );
                });
        });
    ui.end_frame();

    let layout = &ui.layout_engine.result;
    let lr = layout.rect(label.unwrap());
    let vr = layout.rect(value.unwrap());
    assert!(lr.size.w > 0.0, "label cell must have a positive width");
    assert!(
        vr.min.x >= lr.max().x - 0.5,
        "value cell must start at or past the label cell's right edge: \
         label={lr:?}, value={vr:?}",
    );
}
