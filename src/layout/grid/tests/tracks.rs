//! How a track resolves: fixed, fill weights, hug, and the floors under
//! each.

use crate::layout::axis::Axis;
use crate::layout::grid::axis_scratch::AxisScratch;
use crate::layout::grid::axis_scratch::HugRanges;

use crate::layout::intrinsic::LenReq;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel};
use glam::UVec2;

#[test]
fn grid_fixed_and_fill_columns_split_remainder() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fixed(120.0), Track::fill()])
            .rows([Track::fill()])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("left"))
                    .grid_cell((0, 0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("right"))
                    .grid_cell((0, 1))
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(kids[0].size.w, 120.0);
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].size.w, 280.0);
    assert_eq!(kids[1].min.x, 120.0);
    assert_eq!(kids[0].size.h, 200.0);
    assert_eq!(kids[1].size.h, 200.0);
}

#[test]
fn grid_hug_column_takes_max_span1_child_intrinsic() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    // Hug col 0: max(button widths). Buttons measure label at 8px/char +
    // default padding 24 + 2*1 chrome stroke → label_w + 26.
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::hug(), Track::fill()])
            .rows([Track::hug(), Track::hug()])
            .size((Sizing::FILL, Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("short"))
                    .label("ok")
                    .grid_cell((0, 0))
                    .show(ui);
                Button::new()
                    .id(WidgetId::from_hash("long"))
                    .label("hello!!")
                    .grid_cell((1, 0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("body"))
                    .grid_cell((0, 1))
                    .grid_span((2, 1))
                    .show(ui);
            })
            .response
            .node()
    });
    let nodes = h.main_child_ids(root);
    let min_slot = LenReq::MinContent.slot(Axis::X);
    let max_slot = LenReq::MaxContent.slot(Axis::X);
    for node in &nodes[..2] {
        let cached = h.engines.layout.scratch.intrinsics[node.idx()];
        assert!(
            !cached[min_slot].is_nan() && !cached[max_slot].is_nan(),
            "a Hug cell must populate both intrinsic slots in one query",
        );
    }
    let fill_cached = h.engines.layout.scratch.intrinsics[nodes[2].idx()];
    assert!(!fill_cached[min_slot].is_nan());
    assert!(
        fill_cached[max_slot].is_nan(),
        "a Fill cell keeps Stack's single min-content path",
    );
    let kids = h.main_child_rects(root);
    let short_btn = kids[0];
    let long_btn = kids[1];
    let body = kids[2];
    assert_eq!(body.min.x, 82.0);
    assert_eq!(body.size.w, 318.0);
    assert_eq!(short_btn.min.x, 0.0);
    assert_eq!(long_btn.min.x, 0.0);
}

/// A `Hug` grid column whose cells are `FILL`-width hugs to the *widest*
/// cell's content, and every cell stretches to that width. Backs the node
/// editor's value column: each editor fills the column so they're a uniform
/// width, while the column sizes to the longest value (no overflow).
#[test]
fn hug_column_stretches_fill_cells_to_widest_content() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::hug()])
            .rows([Track::hug(), Track::hug()])
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("a"))
                    .grid_cell((0, 0))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("fa"))
                            .size((Sizing::fixed(120.0), Sizing::fixed(20.0)))
                            .show(ui);
                    });
                Panel::hstack()
                    .id(WidgetId::from_hash("b"))
                    .grid_cell((1, 0))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("fb"))
                            .size((Sizing::fixed(60.0), Sizing::fixed(20.0)))
                            .show(ui);
                    });
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(
        kids[0].size.w, 120.0,
        "column hugs to the widest cell's content"
    );
    assert_eq!(
        kids[1].size.w, 120.0,
        "the narrow FILL cell stretched to match"
    );
}

/// A `Hug` column with a `.max()` clamp caps the track. Shrinkable content
/// follows that slot; Fixed content keeps its exact extent and overflows.
#[test]
fn hug_column_max_caps_shrinkable_and_rigid_content() {
    use crate::text::wrap::TextWrap;

    let mut h = UiHarness::new(UVec2::new(600, 200));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::hug().max(150.0)])
            .rows([Track::hug()])
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("btn"))
                    .label("a_very_long_value_label_here")
                    .text_wrap(TextWrap::Ellipsis)
                    .size((Sizing::FILL, Sizing::HUG))
                    .grid_cell((0, 0))
                    .show(ui);
            })
            .response
            .node()
    });
    let btn = h.main_child_rects(root)[0];
    assert_eq!(btn.size.w, 150.0, "hug column capped at its max");

    // The track caps at 150, but the Fixed(200) child remains exact.
    let rigid = rigid_first_col_rects(Track::hug().max(150.0), 100);
    assert_eq!(rigid[0].size.w, 200.0, "Fixed child remains exact");
    assert_eq!(
        rigid[1].min.x, 150.0,
        "the next track starts after the capped Hug track",
    );
}

#[test]
fn grid_fill_weights_and_clamps() {
    type Case = (
        &'static str,
        Track,
        Track,
        f32, // expected col 0 w
        f32, // expected col 1 w
    );
    let cases: &[Case] = &[
        (
            "weights_split_proportionally",
            Track::fill_weight(1.0),
            Track::fill_weight(3.0),
            100.0,
            300.0,
        ),
        (
            "min_clamp_steals_from_other_stars",
            Track::fill_weight(1.0).min(200.0),
            Track::fill_weight(3.0),
            200.0,
            200.0,
        ),
        (
            "max_clamp_donates_to_other_stars",
            Track::fill_weight(3.0).max(150.0),
            Track::fill_weight(1.0),
            150.0,
            250.0,
        ),
        (
            "maximum_finite_weights",
            Track::fill_weight(f32::MAX),
            Track::fill_weight(f32::MAX),
            200.0,
            200.0,
        ),
    ];
    for (label, c0, c1, want0, want1) in cases {
        let mut h = UiHarness::new(UVec2::new(400, 100));
        let root = h.frame_value(|ui| {
            Grid::new()
                .auto_id()
                .cols([*c0, *c1])
                .rows([Track::fill()])
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("a"))
                        .grid_cell((0, 0))
                        .show(ui);
                    Frame::new()
                        .id(WidgetId::from_hash("b"))
                        .grid_cell((0, 1))
                        .show(ui);
                })
                .response
                .node()
        });
        let kids = h.main_child_rects(root);
        assert_eq!(kids[0].size.w, *want0, "case: {label} col0");
        assert_eq!(kids[1].size.w, *want1, "case: {label} col1");
    }

    // The first track caps at 100px and donates the 300px remainder to col 1;
    // its Fixed(200) child overflows without changing track distribution.
    let rigid = rigid_first_col_rects(Track::fill().max(100.0), 400);
    assert_eq!(rigid[0].size.w, 200.0, "Fixed child remains exact");
    assert_eq!(rigid[1].min.x, 100.0, "col 0 track is capped at 100px");
    assert_eq!(rigid[1].size.w, 300.0, "col 1 receives 400 - 100");
}

#[test]
fn grid_fill_col_floors_at_descendant_min_content() {
    // Two equal-weight Fill cols, surface 300 wide. Cell (0,0) holds a
    // Fixed-width 200 frame: that's the col's MinContent intrinsic
    // floor. Without the floor, weights split 150/150 and the rigid
    // frame overflows its cell. With the capped Phase 3 content floor,
    // col 0 clamps to 200 and col 1 takes the 100 remainder — matches
    // Stack's freeze-loop floor.
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fill(), Track::fill()])
            .rows([Track::fill()])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("rigid"))
                    .size((Sizing::fixed(200.0), Sizing::FILL))
                    .grid_cell((0, 0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("flex"))
                    .grid_cell((0, 1))
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(
        kids[0].size.w, 200.0,
        "rigid cell floors at descendant min-content"
    );
    assert_eq!(kids[1].size.w, 100.0, "flex cell takes the remainder");
}

#[test]
fn grid_fill_row_floors_at_descendant_min_content() {
    // Symmetric Y-axis case: two equal-weight Fill rows, surface 100
    // tall. Cell (0,0) holds a Fixed-height 60 frame; cell (1,0) is
    // open. Without floor: rows split 50/50 and the rigid frame
    // overflows. With the floor (Phase 2 records the child's Y
    // min-content into hug_min): row 0 clamps to 60, row 1 takes 40.
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fill()])
            .rows([Track::fill(), Track::fill()])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("rigid"))
                    .size((Sizing::FILL, Sizing::fixed(60.0)))
                    .grid_cell((0, 0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("flex"))
                    .grid_cell((1, 0))
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(
        kids[0].size.h, 60.0,
        "rigid row floors at descendant min-content"
    );
    assert_eq!(kids[1].size.h, 40.0, "flex row takes the remainder");
}

#[test]
fn grid_hug_rows_floor_at_their_measured_height_when_cramped() {
    // Two Hug rows, each holding a Fixed-height 60 frame, in a grid
    // whose own height is fixed at 100 — the one way to cramp rows, since
    // a Fill grid floors its height at its content and never gets here.
    // A Fixed frame's Y min-content is its 60, so each row's range is
    // `[60, 60]` and `hug_min_sum` is 120 against 100 remaining: the solve
    // takes the cramped arm, every row keeps its 60, and the grid
    // overflows by 20.
    //
    // With the min left unwritten the range read `[0, 60]` and the solve
    // took the slack arm instead — 0 + 100 * 60/120 = 50 per row — so
    // row 1 began at y = 50 and both rigid frames overflowed cells that
    // had no reason to shrink.
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fill()])
            .rows([Track::hug(), Track::hug()])
            .size((Sizing::FILL, Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("top"))
                    .size((Sizing::FILL, Sizing::fixed(60.0)))
                    .grid_cell((0, 0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("bottom"))
                    .size((Sizing::FILL, Sizing::fixed(60.0)))
                    .grid_cell((1, 0))
                    .show(ui);
            })
            .response
            .node()
    });
    let kids = h.main_child_rects(root);
    assert_eq!(kids[0].size.h, 60.0, "row 0 keeps its measured height");
    assert_eq!(
        kids[1].min.y, 60.0,
        "row 1 starts past row 0's full height, so the grid overflows"
    );
    assert_eq!(kids[1].size.h, 60.0, "row 1 keeps its measured height");
}

/// Pins implicit contract: `Fixed`/`Hug` resolved, `Fill` unresolved so
/// cells see `INF` (WPF intrinsic trick that defers Fill until arrange).
#[test]
fn resolve_axis_marks_fixed_and_hug_resolved_but_leaves_fill_unresolved() {
    let tracks = [Track::fixed(50.0), Track::hug(), Track::fill()];
    let mut a = AxisScratch::default();
    a.reset_for(tracks.len());
    let hugs = HugRanges {
        min: &[0.0, 10.0, 0.0],
        max: &[0.0, 30.0, 0.0],
    };

    a.resolve_axis(&tracks, hugs, 200.0, 0.0, false);

    assert!(
        a.resolved.contains(0) && a.resolved.contains(1) && !a.resolved.contains(2),
        "Fill cols must stay unresolved so `known_span_size` returns INF for them"
    );
    assert_eq!(
        a.known_span_size(Span::new(0, 2), 10.0),
        50.0 + 10.0 + 30.0,
        "the fully resolved Fixed + Hug span includes its internal gap",
    );
    assert!(
        a.known_span_size(Span::new(0, 3), 10.0).is_infinite(),
        "a span containing unresolved Fill remains uncommitted",
    );
}

/// Pin: each Hug row resolves to its own cells' max desired height,
/// independent of other rows.
#[test]
fn grid_multi_row_hug_heights_resolve_independently() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut grid_node = None;
    let mut kids = Vec::new();
    h.frame(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                grid_node = Some(
                    Grid::new()
                        .id(WidgetId::from_hash("multi-row"))
                        .cols([Track::fixed(50.0)])
                        .rows([Track::hug(), Track::hug(), Track::hug()])
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui, |ui| {
                            kids.push(
                                Frame::new()
                                    .id(WidgetId::from_hash("short"))
                                    .size((50.0, 10.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(),
                            );
                            kids.push(
                                Frame::new()
                                    .id(WidgetId::from_hash("tall"))
                                    .size((50.0, 80.0))
                                    .grid_cell((1, 0))
                                    .show(ui)
                                    .node(),
                            );
                            kids.push(
                                Frame::new()
                                    .id(WidgetId::from_hash("med"))
                                    .size((50.0, 30.0))
                                    .grid_cell((2, 0))
                                    .show(ui)
                                    .node(),
                            );
                        })
                        .response
                        .node(),
                );
            });
    });
    assert_eq!(h.ui.layout(Layer::Main).rect[kids[0].idx()].size.h, 10.0);
    assert_eq!(h.ui.layout(Layer::Main).rect[kids[1].idx()].size.h, 80.0);
    assert_eq!(h.ui.layout(Layer::Main).rect[kids[2].idx()].size.h, 30.0);
    assert_eq!(
        h.layout_rect(WidgetId::from_hash("multi-row"))
            .expect("arranged")
            .size
            .h,
        120.0
    );
}

fn rigid_first_col_rects(first: Track, surface_width: u32) -> Vec<Rect> {
    let mut h = UiHarness::new(UVec2::new(surface_width, 100));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([first, Track::fill()])
            .rows([Track::fill()])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("rigid"))
                    .size((Sizing::fixed(200.0), Sizing::FILL))
                    .grid_cell((0, 0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("flex"))
                    .grid_cell((0, 1))
                    .show(ui);
            })
            .response
            .node()
    });
    h.main_child_rects(root)
}
