//! Cache × full-frame integration: records widget trees across two
//! frames at the same surface and asserts the warm-cache frame
//! reproduces the cold-frame layout (and encoded commands). Catches
//! per-frame engine state we forgot to snapshot/restore on a cache
//! hit.
use crate::primitives::widget_id::WidgetId;

use crate::TextStyle;
use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::forest::tree::NodeId;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::background::Background;
use crate::primitives::shadow::Shadow;
use crate::primitives::{
    color::Color, corners::Corners, stroke::Stroke, transform::TranslateScale,
};
use crate::widgets::{frame::Frame, grid::Grid, panel::Panel, text::Text};
use glam::UVec2;
use std::rc::Rc;

/// Run `record` twice at `size` (cold then warm-from-cache) and assert
/// every captured node's arranged rect matches across the two frames.
/// `record` pushes the nodes whose rects matter into `capture`.
fn assert_warm_rects_match_cold(
    ui: &mut Ui,
    size: UVec2,
    msg: &str,
    mut record: impl FnMut(&mut Ui, &mut Vec<NodeId>),
) {
    let mut cold_nodes = Vec::new();
    ui.run_at_acked(size, |ui| record(ui, &mut cold_nodes));
    let cold: Vec<_> = cold_nodes
        .iter()
        .map(|n| ui.layout[Layer::Main].rect[n.index()])
        .collect();

    let mut warm_nodes = Vec::new();
    ui.run_at_acked(size, |ui| record(ui, &mut warm_nodes));
    let warm: Vec<_> = warm_nodes
        .iter()
        .map(|n| ui.layout[Layer::Main].rect[n.index()])
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
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("g"))
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug()]))
                        .gap_xy(6.0, 16.0)
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("Title:")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(ui),
                            );
                            capture.push(
                                Text::new("value column")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .wrapping()
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node(ui),
                            );
                        });
                });
        }),
        ("nested_grids", |ui, capture| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("outer"))
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug()]))
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("outer-L")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(ui),
                            );
                            Panel::vstack()
                                .id(WidgetId::from_hash("inner-host"))
                                .grid_cell((0, 1))
                                .show(ui, |ui| {
                                    Grid::new()
                                        .id(WidgetId::from_hash("inner"))
                                        .size((Sizing::FILL, Sizing::Hug))
                                        .cols(Rc::from([Track::hug(), Track::hug(), Track::fill()]))
                                        .rows(Rc::from([Track::hug()]))
                                        .show(ui, |ui| {
                                            for (col, label) in [(0, "a"), (1, "bb"), (2, "end")] {
                                                capture.push(
                                                    Text::new(label)
                                                        .id(WidgetId::from_hash((
                                                            "inner-cell",
                                                            col,
                                                        )))
                                                        .style(
                                                            TextStyle::default()
                                                                .with_font_size(14.0),
                                                        )
                                                        .grid_cell((0, col))
                                                        .show(ui)
                                                        .node(ui),
                                                );
                                            }
                                        });
                                });
                        });
                });
        }),
        ("sibling_grids", |ui, capture| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("g1"))
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug()]))
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("L1:")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(ui),
                            );
                            capture.push(
                                Text::new("v1")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node(ui),
                            );
                        });
                    Grid::new()
                        .id(WidgetId::from_hash("g2"))
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug()]))
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("Description:")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(ui),
                            );
                            capture.push(
                                Text::new("end")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 2))
                                    .show(ui)
                                    .node(ui),
                            );
                        });
                });
        }),
    ];
    for (label, record) in cases {
        let mut ui = Ui::for_test_at_text(UVec2::new(800, 600));
        assert_warm_rects_match_cold(
            &mut ui,
            UVec2::new(800, 600),
            &format!("case: {label}"),
            *record,
        );
    }
}

/// Per-driver cache-hit defense. Today only Grid retains per-subtree
/// measure→arrange state (see `LayoutScratch` docs on the cache-hit
/// contract); the other drivers drain their scratch on measure exit
/// so a cache hit at an ancestor is structurally invisible to them.
/// This test pins that property — any future driver that accidentally
/// adds category-(2) state without wiring it through
/// [`LayoutScratch::restore_after_cache_hit`] will desync these
/// fixtures' warm rects from cold.
///
/// Each case builds a minimal subtree under the named driver, runs
/// the same `record` cold then warm at the same surface, and asserts
/// the captured leaf rects match. The cache is shared across the two
/// frames inside one `Ui`, so the second frame's outer Panel cache
/// hit forces the driver's measure to be skipped — if its arrange
/// reads stale or zero state, the warm rect will diverge.
#[test]
fn cache_hit_preserves_per_driver_rects() {
    type Build = fn(&mut Ui, &mut Vec<NodeId>);
    let cases: &[(&str, Build)] = &[
        ("hstack", |ui, capture| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("row"))
                    .gap(6.0)
                    .show(ui, |ui| {
                        for (i, label) in ["alpha", "beta", "gamma"].iter().enumerate() {
                            capture.push(
                                Text::new(*label)
                                    .id(WidgetId::from_hash(("cell", i)))
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .show(ui)
                                    .node(ui),
                            );
                        }
                    });
            });
        }),
        ("vstack_fill_freeze", |ui, capture| {
            // Three Fill children with min-content floors that force
            // the freeze loop. Stack measure pushes onto
            // `stack_fill.pool`; a cache hit at the outer panel skips
            // the freeze entirely, so arrange must still read correct
            // per-child slots from `desired` alone.
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("freeze"))
                    .size((Sizing::Fixed(200.0), Sizing::Hug))
                    .show(ui, |ui| {
                        for (i, label) in [
                            "needs-some-room-here",
                            "wider-than-share-A",
                            "wider-than-share-B",
                        ]
                        .iter()
                        .enumerate()
                        {
                            capture.push(
                                Text::new(*label)
                                    .id(WidgetId::from_hash(("fill", i)))
                                    .size((Sizing::Fill(1.0), Sizing::Hug))
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .show(ui)
                                    .node(ui),
                            );
                        }
                    });
            });
        }),
        ("wrap_hstack", |ui, capture| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::wrap_hstack()
                    .id(WidgetId::from_hash("wrap"))
                    .size((Sizing::Fixed(120.0), Sizing::Hug))
                    .gap(4.0)
                    .line_gap(4.0)
                    .show(ui, |ui| {
                        for (i, label) in ["aa", "bbb", "cccc", "dd", "eeeee", "ff"]
                            .iter()
                            .enumerate()
                        {
                            capture.push(
                                Text::new(*label)
                                    .id(WidgetId::from_hash(("tag", i)))
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .show(ui)
                                    .node(ui),
                            );
                        }
                    });
            });
        }),
        ("zstack", |ui, capture| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("z"))
                    .size((Sizing::Fixed(160.0), Sizing::Fixed(40.0)))
                    .show(ui, |ui| {
                        for (i, label) in ["under", "over"].iter().enumerate() {
                            capture.push(
                                Text::new(*label)
                                    .id(WidgetId::from_hash(("layer", i)))
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .show(ui)
                                    .node(ui),
                            );
                        }
                    });
            });
        }),
        ("canvas", |ui, capture| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("c"))
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
                    .show(ui, |ui| {
                        for (i, label, pos) in [
                            (0, "tl", glam::Vec2::new(4.0, 4.0)),
                            (1, "br", glam::Vec2::new(80.0, 40.0)),
                        ] {
                            capture.push(
                                Text::new(label)
                                    .id(WidgetId::from_hash(("pin", i)))
                                    .position(pos)
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .show(ui)
                                    .node(ui),
                            );
                        }
                    });
            });
        }),
    ];
    for (label, record) in cases {
        let mut ui = Ui::for_test_at_text(UVec2::new(800, 600));
        assert_warm_rects_match_cold(
            &mut ui,
            UVec2::new(800, 600),
            &format!("case: {label}"),
            *record,
        );
    }
}

/// Cache-correctness generalization: a measure-cache hit must not
/// perturb ANY downstream consumer of per-frame engine state — so a
/// fully-encoded `RenderCmdBuffer` from a warm frame must be
/// byte-identical to one from a cold frame.
#[test]
fn encoded_buffer_stable_across_cache_hit_boundary() {
    let record = |ui: &mut Ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(8.0)
            .gap(6.0)
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("transformed"))
                    .transform(TranslateScale::new(glam::Vec2::new(4.0, 2.0), 1.0))
                    .clip_rect()
                    .size((Sizing::FILL, Sizing::Hug))
                    .padding(6.0)
                    .background(Background {
                        fill: Color::rgb(0.16, 0.18, 0.22).into(),
                        stroke: Stroke::solid(Color::rgb(0.3, 0.34, 0.42), 1.0),
                        radius: Corners::all(4.0),
                        shadow: Shadow::NONE,
                    })
                    .show(ui, |ui| {
                        Grid::new()
                            .id(WidgetId::from_hash("grid"))
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug(), Track::hug()]))
                            .gap_xy(6.0, 8.0)
                            .show(ui, |ui| {
                                Text::new("Title:")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((0, 0))
                                    .show(ui);
                                Text::new(
                                    "The quick brown fox jumps over the lazy dog. \
                                     Pack my box with five dozen liquor jugs.",
                                )
                                .auto_id()
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui);
                                Text::new("Tag:")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .grid_cell((1, 0))
                                    .show(ui);
                                Text::new("layout, grid, intrinsic, wrapping")
                                    .auto_id()
                                    .style(TextStyle::default().with_font_size(14.0))
                                    .wrapping()
                                    .grid_cell((1, 1))
                                    .show(ui);
                            });
                    });
                Frame::new()
                    .id(WidgetId::from_hash("under"))
                    .size((Sizing::FILL, Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.4, 0.4, 0.5).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };

    let mut ui = Ui::for_test_at_text(UVec2::new(800, 600));
    ui.run_at_acked(UVec2::new(800, 600), |ui| record(ui));
    let cold = ui.encode_cmds();

    ui.run_at_acked(UVec2::new(800, 600), |ui| record(ui));
    let warm = ui.encode_cmds();

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
    let record = |ui: &mut Ui, capture: &mut Vec<NodeId>| {
        capture.clear();
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("xform"))
                    .transform(TranslateScale::new(glam::Vec2::new(2.0, 2.0), 1.0))
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Grid::new()
                            .id(WidgetId::from_hash("g"))
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug()]))
                            .show(ui, |ui| {
                                capture.push(
                                    Text::new("Title:")
                                        .auto_id()
                                        .style(TextStyle::default().with_font_size(14.0))
                                        .grid_cell((0, 0))
                                        .show(ui)
                                        .node(ui),
                                );
                                capture.push(
                                    Text::new(
                                        "Lorem ipsum dolor sit amet, consectetur \
                                         adipiscing elit, sed do eiusmod tempor.",
                                    )
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
    };

    let mut ui = Ui::for_test();
    let widths = [800u32, 800, 600, 800, 600, 600, 800, 1000, 600];
    for (i, &w) in widths.iter().enumerate() {
        let mut warm_nodes = Vec::new();
        ui.run_at_acked(UVec2::new(w, 600), |ui| {
            record(ui, &mut warm_nodes);
        });
        let warm_rects: Vec<_> = warm_nodes
            .iter()
            .map(|n| ui.layout[Layer::Main].rect[n.index()])
            .collect();

        ui.clear_measure_cache();
        let mut cold_nodes = Vec::new();
        ui.run_at_acked(UVec2::new(w, 600), |ui| {
            record(ui, &mut cold_nodes);
        });
        let cold_rects: Vec<_> = cold_nodes
            .iter()
            .map(|n| ui.layout[Layer::Main].rect[n.index()])
            .collect();

        assert_eq!(
            warm_rects, cold_rects,
            "step {i}: warm-cache rects diverged from cold remeasure at width={w}",
        );
    }
}
