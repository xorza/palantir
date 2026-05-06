//! Cache × full-frame integration: builds widget trees across two
//! frames at the same surface and asserts the warm-cache frame
//! reproduces the cold-frame layout (and encoded commands). Catches
//! per-frame engine state we forgot to snapshot/restore on a cache
//! hit.

use crate::TextStyle;
use crate::Ui;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::{
    color::Color, corners::Corners, stroke::Stroke, transform::TranslateScale,
};
use crate::support::testing::{begin, encode_cmds, new_ui_text, ui_with_text};
use crate::tree::NodeId;
use crate::tree::element::Configure;
use crate::widgets::theme::{Background, Surface};
use crate::widgets::{frame::Frame, grid::Grid, panel::Panel, text::Text};
use glam::UVec2;
use std::rc::Rc;

/// Run `build` twice at `size` (cold then warm-from-cache) and assert
/// every captured node's arranged rect matches across the two frames.
/// `build` pushes the nodes whose rects matter into `capture`.
fn assert_warm_rects_match_cold(
    ui: &mut Ui,
    size: UVec2,
    msg: &str,
    mut build: impl FnMut(&mut Ui, &mut Vec<NodeId>),
) {
    let mut cold_nodes = Vec::new();
    build(ui, &mut cold_nodes);
    ui.end_frame();
    let cold: Vec<_> = cold_nodes
        .iter()
        .map(|n| ui.pipeline.layout.result.rect[n.index()])
        .collect();

    begin(ui, size);
    let mut warm_nodes = Vec::new();
    build(ui, &mut warm_nodes);
    ui.end_frame();
    let warm: Vec<_> = warm_nodes
        .iter()
        .map(|n| ui.pipeline.layout.result.rect[n.index()])
        .collect();

    assert_eq!(cold, warm, "{msg}");
}

/// Cross-frame measure-cache regression. When the cache hits at a
/// Grid (or any ancestor), the grid driver's per-frame `GridHugStore`
/// scratch must be re-populated from the snapshot — otherwise arrange
/// computes zero column widths, collapsing every cell to x=0.
///
/// Topologies pinned: a single grid, nested grids (outer + inner), and
/// two sibling grids inside a vstack (cache hit must restore hugs for
/// both, in pre-order).
#[test]
fn cache_hit_preserves_grid_cell_rects() {
    type Build = fn(&mut Ui, &mut Vec<NodeId>);
    let cases: &[(&str, Build)] = &[
        ("single_grid", |ui, capture| {
            Panel::vstack()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .with_id("g")
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug()]))
                        .gap_xy(6.0, 16.0)
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("Title:")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node,
                            );
                            capture.push(
                                Text::new("value column")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .wrapping()
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node,
                            );
                        });
                });
        }),
        ("nested_grids", |ui, capture| {
            Panel::vstack()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .with_id("outer")
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug()]))
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("outer-L")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node,
                            );
                            Panel::vstack()
                                .with_id("inner-host")
                                .grid_cell((0, 1))
                                .show(ui, |ui| {
                                    Grid::new()
                                        .with_id("inner")
                                        .size((Sizing::FILL, Sizing::Hug))
                                        .cols(Rc::from([Track::hug(), Track::hug(), Track::fill()]))
                                        .rows(Rc::from([Track::hug()]))
                                        .show(ui, |ui| {
                                            for (col, label) in [(0, "a"), (1, "bb"), (2, "end")] {
                                                capture.push(
                                                    Text::new(label)
                                                        .with_id(("inner-cell", col))
                                                        .style(
                                                            TextStyle::default()
                                                                .with_font_size(14.0),
                                                        )
                                                        .grid_cell((0, col))
                                                        .show(ui)
                                                        .node,
                                                );
                                            }
                                        });
                                });
                        });
                });
        }),
        ("sibling_grids", |ui, capture| {
            Panel::vstack()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .with_id("g1")
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug()]))
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("L1:")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node,
                            );
                            capture.push(
                                Text::new("v1")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node,
                            );
                        });
                    Grid::new()
                        .with_id("g2")
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug()]))
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("Description:")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node,
                            );
                            capture.push(
                                Text::new("end")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 2))
                                    .show(ui)
                                    .node,
                            );
                        });
                });
        }),
    ];
    for (label, build) in cases {
        let mut ui = ui_with_text(UVec2::new(800, 600));
        assert_warm_rects_match_cold(
            &mut ui,
            UVec2::new(800, 600),
            &format!("case: {label}"),
            *build,
        );
    }
}

/// Cache-correctness generalization: a measure-cache hit must not
/// perturb ANY downstream consumer of per-frame engine state — so a
/// fully-encoded `RenderCmdBuffer` from a warm frame must be
/// byte-identical to one from a cold frame.
#[test]
fn encoded_buffer_stable_across_cache_hit_boundary() {
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(8.0)
            .gap(6.0)
            .show(ui, |ui| {
                Panel::zstack()
                    .with_id("transformed")
                    .transform(TranslateScale::new(glam::Vec2::new(4.0, 2.0), 1.0))
                    .background(Surface::scissor())
                    .size((Sizing::FILL, Sizing::Hug))
                    .padding(6.0)
                    .background(Background {
                        fill: Color::rgb(0.16, 0.18, 0.22),
                        stroke: Some(Stroke {
                            width: 1.0,
                            color: Color::rgb(0.3, 0.34, 0.42),
                        }),
                        radius: Corners::all(4.0),
                    })
                    .show(ui, |ui| {
                        Grid::new()
                            .with_id("grid")
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug(), Track::hug()]))
                            .gap_xy(6.0, 8.0)
                            .show(ui, |ui| {
                                Text::new("Title:")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui);
                                Text::new(
                                    "The quick brown fox jumps over the lazy dog. \
                                     Pack my box with five dozen liquor jugs.",
                                )
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui);
                                Text::new("Tag:")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((1, 0))
                                    .show(ui);
                                Text::new("layout, grid, intrinsic, wrapping")
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .wrapping()
                                    .grid_cell((1, 1))
                                    .show(ui);
                            });
                    });
                Frame::new()
                    .with_id("under")
                    .size((Sizing::FILL, Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.4, 0.4, 0.5),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };

    let mut ui = ui_with_text(UVec2::new(800, 600));
    build(&mut ui);
    ui.end_frame();
    let cold = encode_cmds(&ui);

    begin(&mut ui, UVec2::new(800, 600));
    build(&mut ui);
    ui.end_frame();
    let warm = encode_cmds(&ui);

    assert_eq!(cold.kinds, warm.kinds, "cmd kind sequence must match");
    assert_eq!(cold.starts, warm.starts, "cmd payload offsets must match");
    assert_eq!(cold.data, warm.data, "cmd payload bytes must match");
}

/// Stress test: alternating surface widths force the cache through
/// repeated hit/replace transitions. At each step, the warm cache's
/// rects must equal what a cold remeasure produces — a forced miss
/// via `internals::clear_measure_cache()` is the ground-truth oracle.
#[test]
fn cache_rects_match_cold_oracle_across_width_changes() {
    let build = |ui: &mut Ui, capture: &mut Vec<NodeId>| {
        capture.clear();
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .with_id("xform")
                    .transform(TranslateScale::new(glam::Vec2::new(2.0, 2.0), 1.0))
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Grid::new()
                            .with_id("g")
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug()]))
                            .show(ui, |ui| {
                                capture.push(
                                    Text::new("Title:")
                                        .style(TextStyle::default().with_font_size(14.0))
                                        .grid_cell((0, 0))
                                        .show(ui)
                                        .node,
                                );
                                capture.push(
                                    Text::new(
                                        "Lorem ipsum dolor sit amet, consectetur \
                                         adipiscing elit, sed do eiusmod tempor.",
                                    )
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .wrapping()
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node,
                                );
                            });
                    });
            });
    };

    let mut ui = new_ui_text();
    let widths = [800u32, 800, 600, 800, 600, 600, 800, 1000, 600];
    for (i, &w) in widths.iter().enumerate() {
        begin(&mut ui, UVec2::new(w, 600));
        let mut warm_nodes = Vec::new();
        build(&mut ui, &mut warm_nodes);
        ui.end_frame();
        let warm_rects: Vec<_> = warm_nodes
            .iter()
            .map(|n| ui.pipeline.layout.result.rect[n.index()])
            .collect();

        crate::support::internals::clear_measure_cache(&mut ui);
        begin(&mut ui, UVec2::new(w, 600));
        let mut cold_nodes = Vec::new();
        build(&mut ui, &mut cold_nodes);
        ui.end_frame();
        let cold_rects: Vec<_> = cold_nodes
            .iter()
            .map(|n| ui.pipeline.layout.result.rect[n.index()])
            .collect();

        assert_eq!(
            warm_rects, cold_rects,
            "step {i}: warm-cache rects diverged from cold remeasure at width={w}",
        );
    }
}
