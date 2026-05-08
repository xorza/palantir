//! Showcase regressions where two cells from a Grid (or two
//! back-to-back grids inside a vstack) ended up painting on top of
//! each other. Pinned via arranged-rect order plus a render-pass
//! check on emitted `DrawText` x positions.

use crate::TextStyle;
use crate::Ui;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::{color::Color, corners::Corners, stroke::Stroke};
use crate::renderer::frontend::cmd_buffer::{CmdKind, DrawTextPayload};
use crate::support::testing::{encode_cmds, ui_with_text};
use crate::tree::Layer;
use crate::tree::element::Configure;
use crate::widgets::theme::Background;
use crate::widgets::{grid::Grid, panel::Panel, text::Text};
use glam::UVec2;
use std::rc::Rc;

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

fn section(ui: &mut Ui, id: &'static str, body: &mut dyn FnMut(&mut Ui)) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(6.0)
        .padding(8.0)
        .background(Background {
            fill: Color::rgb(0.16, 0.18, 0.22),
            stroke: Some(Stroke {
                width: 1.0,
                color: Color::rgb(0.30, 0.34, 0.42),
            }),
            radius: Corners::all(4.0),
        })
        .show(ui, |ui| {
            Text::new("title")
                .id_salt(("section-title", id))
                .style(TextStyle::default().with_font_size(12.0))
                .show(ui);
            body(ui);
        });
}

/// Showcase regressions: two cells in a Grid with a wrapping text column
/// must not paint on top of each other. Pinned across two topologies:
/// a default-sized Grid with two Hug cols, and a FILL-sized Grid with
/// Hug + Fill cols (the property-grid pattern).
#[test]
fn grid_columns_with_wrapping_text_do_not_overlap() {
    type Case = (&'static str, Option<Sizing>, [Track; 2], (f32, f32));
    let cases: &[Case] = &[
        (
            "two_hug_columns",
            None,
            [Track::hug(), Track::hug()],
            (0.0, 0.0),
        ),
        (
            "hug_label_fill_value",
            Some(Sizing::FILL),
            [Track::hug(), Track::fill()],
            (6.0, 16.0),
        ),
    ];
    let long_text = "The quick brown fox jumps over the lazy dog. Pack my box \
                     with five dozen liquor jugs. How vexingly quick daft zebras jump!";
    for (label_id, grid_main, cols, gap_xy) in cases {
        let mut ui = ui_with_text(UVec2::new(800, 600));
        let mut left = None;
        let mut right = None;
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(&mut ui, |ui| {
                let mut g = Grid::new();
                if let Some(s) = *grid_main {
                    g = g.size((s, Sizing::Hug));
                }
                g.cols(Rc::from(*cols))
                    .rows(Rc::from([Track::hug()]))
                    .gap_xy(gap_xy.0, gap_xy.1)
                    .show(ui, |ui| {
                        left = Some(
                            Text::new(long_text)
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        right = Some(
                            Text::new("right column")
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });
        ui.end_frame();

        let layout = &ui.layout.result[Layer::Main];
        let lr = layout.rect[left.unwrap().index()];
        let rr = layout.rect[right.unwrap().index()];
        assert!(lr.size.w > 0.0, "case: {label_id} left col width");
        assert!(
            rr.min.x >= lr.max().x - 0.5,
            "case: {label_id} right must start past left.right; left={lr:?}, right={rr:?}",
        );
    }
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
                Grid::new()
                    .id_salt("two-hug-inner")
                    .cols(Rc::from([Track::hug(), Track::hug()]))
                    .rows(Rc::from([Track::hug()]))
                    .gap_xy(0.0, 16.0)
                    .show(ui, |ui| {
                        hug_left = Some(
                            Text::new(PARAGRAPH)
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        hug_right = Some(
                            Text::new("right column")
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });

            section(ui, "property-grid", &mut |ui| {
                Grid::new()
                    .id_salt("property-grid-inner")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                    .gap_xy(6.0, 16.0)
                    .show(ui, |ui| {
                        prop_label = Some(
                            Text::new("Title:")
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        prop_value = Some(
                            Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });
        });
    ui.end_frame();

    let layout = &ui.layout.result[Layer::Main];
    let l1 = layout.rect[hug_left.unwrap().index()];
    let r1 = layout.rect[hug_right.unwrap().index()];
    let l2 = layout.rect[prop_label.unwrap().index()];
    let r2 = layout.rect[prop_value.unwrap().index()];

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
            Grid::new()
                .id_salt("property-grid-inner")
                .size((Sizing::FILL, Sizing::Hug))
                .cols(Rc::from([Track::hug(), Track::fill()]))
                .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                .gap_xy(6.0, 16.0)
                .show(ui, |ui| {
                    Text::new("Title:")
                        .style(TextStyle::default().with_font_size(14.0))
                        .grid_cell((0, 0))
                        .show(ui);
                    Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                        .style(TextStyle::default().with_font_size(14.0))
                        .wrapping()
                        .grid_cell((0, 1))
                        .show(ui);
                    Text::new("Description:")
                        .style(TextStyle::default().with_font_size(14.0))
                        .grid_cell((1, 0))
                        .show(ui);
                });
        });
    ui.end_frame();

    let cmds = encode_cmds(&ui);
    let mut text_xs: Vec<f32> = Vec::new();
    for i in 0..cmds.kinds.len() {
        if cmds.kinds[i] == CmdKind::DrawText {
            text_xs.push(cmds.read::<DrawTextPayload>(cmds.starts[i]).rect.min.x);
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
                                Grid::new().id_salt("two-hug-inner")
                                    .cols(Rc::from([Track::hug(), Track::hug()]))
                                    .rows(Rc::from([Track::hug()]))
                                    .gap_xy(0.0, 16.0)
                                    .show(ui, |ui| {
                                        Text::new(PARAGRAPH)
                                            .style(TextStyle::default().with_font_size(14.0))
                                            .wrapping()
                                            .grid_cell((0, 0))
                                            .show(ui);
                                        Text::new("right column")
                                            .style(TextStyle::default().with_font_size(14.0))
                                            .grid_cell((0, 1))
                                            .show(ui);
                                    });
                            });
                            section(ui, "property-grid", &mut |ui| {
                                Grid::new().id_salt("property-grid-inner")
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .cols(Rc::from([Track::hug(), Track::fill()]))
                                    .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                                    .gap_xy(6.0, 16.0)
                                    .show(ui, |ui| {
                                        Text::new("Title:")
                                            .style(TextStyle::default().with_font_size(14.0))
                                            .grid_cell((0, 0))
                                            .show(ui);
                                        Text::new(
                                            "Lorem Ipsum is simply dummy text of the printing industry.",
                                        )
                                        .style(TextStyle::default().with_font_size(14.0))
                                        .wrapping()
                                        .grid_cell((0, 1))
                                        .show(ui);
                                        Text::new("Description:")
                                            .style(TextStyle::default().with_font_size(14.0))
                                            .grid_cell((1, 0))
                                            .show(ui);
                                        Text::new(PARAGRAPH)
                                            .style(TextStyle::default().with_font_size(14.0))
                                            .wrapping()
                                            .grid_cell((1, 1))
                                            .show(ui);
                                        Text::new("Tags:")
                                            .style(TextStyle::default().with_font_size(14.0))
                                            .grid_cell((2, 0))
                                            .show(ui);
                                        Text::new("layout, grid, intrinsic, wrapping, css")
                                            .style(TextStyle::default().with_font_size(14.0))
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
        if cmds.kinds[i] == CmdKind::DrawText {
            let p: DrawTextPayload = cmds.read(cmds.starts[i]);
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
