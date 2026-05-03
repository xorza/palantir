//! Cache × full-frame integration: builds widget trees across two
//! frames at the same surface and asserts the warm-cache frame
//! reproduces the cold-frame layout (and encoded commands). Catches
//! per-frame engine state we forgot to snapshot/restore on a cache
//! hit.

use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Color, Sizing, Stroke, Track, TranslateScale};
use crate::test_support::{begin, encode_cmds, new_ui_text, ui_with_text};
use crate::tree::NodeId;
use crate::widgets::{Frame, Grid, Panel, Styled, Text};
use glam::UVec2;
use std::rc::Rc;

/// Cross-frame measure-cache regression: when the cache hits at a
/// Grid (or any ancestor of a Grid), the grid driver's per-frame
/// `GridHugStore` scratch — populated by `grid::measure` and read
/// by `grid::arrange` — stays at its `reset_for`-zero state because
/// measure was short-circuited. Arrange then computes zero column
/// widths, collapsing every cell to x=0.
#[test]
fn grid_cells_arranged_correctly_on_cache_hit_frame() {
    let build = |ui: &mut Ui, capture: &mut Option<(NodeId, NodeId)>| {
        let mut left = None;
        let mut right = None;
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::with_id("g")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug()]))
                    .gap_xy(6.0, 16.0)
                    .show(ui, |ui| {
                        left = Some(
                            Text::new("Title:")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        right = Some(
                            Text::new("value column")
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });
        *capture = Some((left.unwrap(), right.unwrap()));
    };

    let mut ui = ui_with_text(UVec2::new(800, 600));
    let mut nodes = None;
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let (l, r) = nodes.unwrap();
    let cold_l = ui.layout_engine.result().rect(l);
    let cold_r = ui.layout_engine.result().rect(r);

    begin(&mut ui, UVec2::new(800, 600));
    let mut nodes = None;
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let (l, r) = nodes.unwrap();
    let warm_l = ui.layout_engine.result().rect(l);
    let warm_r = ui.layout_engine.result().rect(r);

    assert_eq!(
        cold_l, warm_l,
        "cache-hit frame must not perturb left-cell rect: cold={cold_l:?} warm={warm_l:?}",
    );
    assert_eq!(
        cold_r, warm_r,
        "cache-hit frame must not perturb right-cell rect: cold={cold_r:?} warm={warm_r:?}",
    );
}

/// Nested grids: outer Grid with an inner Grid in one of its cells.
/// A cache hit at any ancestor must restore hugs for both, each at
/// its current-frame `idx`.
#[test]
fn cache_hit_restores_hugs_for_nested_grids() {
    let build = |ui: &mut Ui, capture: &mut [Option<NodeId>; 4]| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::with_id("outer")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug()]))
                    .show(ui, |ui| {
                        capture[0] = Some(
                            Text::new("outer-L")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        Panel::vstack_with_id("inner-host")
                            .grid_cell((0, 1))
                            .show(ui, |ui| {
                                Grid::with_id("inner")
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .cols(Rc::from([Track::hug(), Track::hug(), Track::fill()]))
                                    .rows(Rc::from([Track::hug()]))
                                    .show(ui, |ui| {
                                        capture[1] = Some(
                                            Text::new("a")
                                                .size_px(14.0)
                                                .grid_cell((0, 0))
                                                .show(ui)
                                                .node,
                                        );
                                        capture[2] = Some(
                                            Text::new("bb")
                                                .size_px(14.0)
                                                .grid_cell((0, 1))
                                                .show(ui)
                                                .node,
                                        );
                                        capture[3] = Some(
                                            Text::new("end")
                                                .size_px(14.0)
                                                .grid_cell((0, 2))
                                                .show(ui)
                                                .node,
                                        );
                                    });
                            });
                    });
            });
    };

    let mut ui = ui_with_text(UVec2::new(800, 600));
    let mut nodes = [None; 4];
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let cold: Vec<_> = nodes
        .iter()
        .map(|n| ui.layout_engine.result().rect(n.unwrap()))
        .collect();

    begin(&mut ui, UVec2::new(800, 600));
    let mut nodes = [None; 4];
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let warm: Vec<_> = nodes
        .iter()
        .map(|n| ui.layout_engine.result().rect(n.unwrap()))
        .collect();

    assert_eq!(
        cold, warm,
        "cache-hit frame must preserve outer+nested grid cell rects",
    );
}

/// Two sibling Grids inside a vstack: a cache hit at the vstack
/// must restore hug arrays for *both* grids, in pre-order.
#[test]
fn cache_hit_restores_hugs_for_multiple_sibling_grids() {
    let build = |ui: &mut Ui, capture: &mut [Option<NodeId>; 4]| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::with_id("g1")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug()]))
                    .show(ui, |ui| {
                        capture[0] = Some(
                            Text::new("L1:")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        capture[1] = Some(
                            Text::new("v1")
                                .size_px(14.0)
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
                Grid::with_id("g2")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug()]))
                    .show(ui, |ui| {
                        capture[2] = Some(
                            Text::new("Description:")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        capture[3] = Some(
                            Text::new("end")
                                .size_px(14.0)
                                .grid_cell((0, 2))
                                .show(ui)
                                .node,
                        );
                    });
            });
    };

    let mut ui = ui_with_text(UVec2::new(800, 600));
    let mut nodes = [None; 4];
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let cold: Vec<_> = nodes
        .iter()
        .map(|n| ui.layout_engine.result().rect(n.unwrap()))
        .collect();

    begin(&mut ui, UVec2::new(800, 600));
    let mut nodes = [None; 4];
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let warm: Vec<_> = nodes
        .iter()
        .map(|n| ui.layout_engine.result().rect(n.unwrap()))
        .collect();

    assert_eq!(
        cold, warm,
        "cache-hit frame must preserve all sibling-grid cell rects"
    );
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
                Panel::zstack_with_id("transformed")
                    .transform(TranslateScale::new(glam::Vec2::new(4.0, 2.0), 1.0))
                    .clip(true)
                    .size((Sizing::FILL, Sizing::Hug))
                    .padding(6.0)
                    .fill(Color::rgb(0.16, 0.18, 0.22))
                    .stroke(Stroke {
                        width: 1.0,
                        color: Color::rgb(0.3, 0.34, 0.42),
                    })
                    .radius(4.0)
                    .show(ui, |ui| {
                        Grid::with_id("grid")
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug(), Track::hug()]))
                            .gap_xy(6.0, 8.0)
                            .show(ui, |ui| {
                                Text::new("Title:").size_px(14.0).grid_cell((0, 0)).show(ui);
                                Text::new(
                                    "The quick brown fox jumps over the lazy dog. \
                                     Pack my box with five dozen liquor jugs.",
                                )
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui);
                                Text::new("Tag:").size_px(14.0).grid_cell((1, 0)).show(ui);
                                Text::new("layout, grid, intrinsic, wrapping")
                                    .size_px(14.0)
                                    .wrapping()
                                    .grid_cell((1, 1))
                                    .show(ui);
                            });
                    });
                Frame::with_id("under")
                    .size((Sizing::FILL, Sizing::Fixed(20.0)))
                    .fill(Color::rgb(0.4, 0.4, 0.5))
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

    assert_eq!(
        cold.kinds, warm.kinds,
        "cmd kind sequence must match across cache-hit boundary",
    );
    assert_eq!(
        cold.starts, warm.starts,
        "cmd payload offsets must match across cache-hit boundary",
    );
    assert_eq!(
        cold.data, warm.data,
        "cmd payload bytes must match across cache-hit boundary",
    );
}

/// Stress test: alternating surface widths force the cache through
/// repeated hit/replace transitions. At each step, the warm cache's
/// rects must equal what a cold remeasure produces — a forced miss
/// via `__clear_measure_cache()` is the ground-truth oracle.
#[test]
fn cache_rects_match_cold_oracle_across_width_changes() {
    let build = |ui: &mut Ui, capture: &mut Vec<NodeId>| {
        capture.clear();
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack_with_id("xform")
                    .transform(TranslateScale::new(glam::Vec2::new(2.0, 2.0), 1.0))
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Grid::with_id("g")
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug()]))
                            .show(ui, |ui| {
                                capture.push(
                                    Text::new("Title:")
                                        .size_px(14.0)
                                        .grid_cell((0, 0))
                                        .show(ui)
                                        .node,
                                );
                                capture.push(
                                    Text::new(
                                        "Lorem ipsum dolor sit amet, consectetur \
                                     adipiscing elit, sed do eiusmod tempor.",
                                    )
                                    .size_px(14.0)
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
            .map(|n| ui.layout_engine.result().rect(*n))
            .collect();

        ui.__clear_measure_cache();
        begin(&mut ui, UVec2::new(w, 600));
        let mut cold_nodes = Vec::new();
        build(&mut ui, &mut cold_nodes);
        ui.end_frame();
        let cold_rects: Vec<_> = cold_nodes
            .iter()
            .map(|n| ui.layout_engine.result().rect(*n))
            .collect();

        assert_eq!(
            warm_rects, cold_rects,
            "step {i}: warm-cache rects diverged from cold remeasure at width={w}",
        );
    }
}
