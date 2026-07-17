use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::layer::Layer;
use crate::forest::tree::node::NodeId;
use crate::layout::grid::{AxisScratch, GridDepthStack, resolve_axis};
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel};
use glam::UVec2;
use std::rc::Rc;

fn child_rects(ui: &Ui, root: NodeId) -> Vec<Rect> {
    ui.forest.trees[Layer::Main]
        .children(root)
        .map(|c| ui.layout[Layer::Main].rect[c.id.idx()])
        .collect()
}

fn rigid_first_col_rects(first: Track, surface_width: u32) -> Vec<Rect> {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(surface_width, 100), |ui| {
        root = Some(
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
                .node(),
        );
    });
    child_rects(&ui, root.unwrap())
}

#[test]
#[should_panic(expected = "GridDepthStack::exit underflow")]
fn grid_depth_stack_rejects_exit_without_enter() {
    GridDepthStack::default().exit();
}

#[test]
fn grid_fixed_and_fill_columns_split_remainder() {
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(400, 200), |ui| {
        root = Some(
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
                .node(),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    assert_eq!(kids[0].size.w, 120.0);
    assert_eq!(kids[0].min.x, 0.0);
    assert_eq!(kids[1].size.w, 280.0);
    assert_eq!(kids[1].min.x, 120.0);
    assert_eq!(kids[0].size.h, 200.0);
    assert_eq!(kids[1].size.h, 200.0);
}

#[test]
fn grid_hug_column_takes_max_span1_child_intrinsic() {
    let mut ui = Ui::for_test();
    let mut root = None;
    // Hug col 0: max(button widths). Buttons measure label at 8px/char +
    // default padding 24 + 2*1 chrome stroke → label_w + 26.
    ui.run_at(UVec2::new(400, 200), |ui| {
        root = Some(
            Grid::new()
                .auto_id()
                .cols([Track::hug(), Track::fill()])
                .rows([Track::hug(), Track::hug()])
                .size((Sizing::FILL, Sizing::FILL))
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
                .node(),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
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
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(400, 200), |ui| {
        root = Some(
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
                .node(),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
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
    use crate::shape::TextWrap;

    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(600, 200), |ui| {
        root = Some(
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
                .node(),
        );
    });
    let btn = child_rects(&ui, root.unwrap())[0];
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
        let mut ui = Ui::for_test();
        let mut root = None;
        ui.run_at(UVec2::new(400, 100), |ui| {
            root = Some(
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
                    .node(),
            );
        });
        let kids = child_rects(&ui, root.unwrap());
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
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(300, 100), |ui| {
        root = Some(
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
                .node(),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
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
    // overflows. With floor (Phase 2 records `d.h` into hug_min for
    // Fill rows): row 0 clamps to 60, row 1 takes 40.
    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(100, 100), |ui| {
        root = Some(
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
                .node(),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    assert_eq!(
        kids[0].size.h, 60.0,
        "rigid row floors at descendant min-content"
    );
    assert_eq!(kids[1].size.h, 40.0, "flex row takes the remainder");
}

#[test]
fn grid_span_covers_multiple_tracks_with_gap() {
    // 3 fixed primary tracks of 100 with gap 10 → spanning all = 320.
    // Body sits in track (1,1) → 110 offset on primary, 50 on secondary.
    let cases: &[(&str, bool)] = &[("col_span", false), ("row_span", true)];
    for (label, swap) in cases {
        let surface = if *swap {
            UVec2::new(200, 400)
        } else {
            UVec2::new(400, 200)
        };
        let mut ui = Ui::for_test();
        let mut root = None;
        ui.run_at(surface, |ui| {
            let primary = [
                Track::fixed(100.0),
                Track::fixed(100.0),
                Track::fixed(100.0),
            ];
            let secondary = [Track::fixed(40.0), Track::fixed(40.0)];
            let mut g = Grid::new().auto_id();
            if *swap {
                g = g.rows(primary).cols(secondary);
            } else {
                g = g.cols(primary).rows(secondary);
            }
            let span = if *swap { (3, 1) } else { (1, 3) };
            root = Some(
                g.gap(10.0)
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("header"))
                            .grid_cell((0, 0))
                            .grid_span(span)
                            .show(ui);
                        Frame::new()
                            .id(WidgetId::from_hash("body"))
                            .grid_cell((1, 1))
                            .show(ui);
                    })
                    .node(),
            );
        });
        let kids = child_rects(&ui, root.unwrap());
        let header = kids[0];
        let body = kids[1];
        let (h_pri_min, h_pri_size, h_sec_size) = if *swap {
            (header.min.y, header.size.h, header.size.w)
        } else {
            (header.min.x, header.size.w, header.size.h)
        };
        let (b_pri_min, b_sec_min, b_pri_size, b_sec_size) = if *swap {
            (body.min.y, body.min.x, body.size.h, body.size.w)
        } else {
            (body.min.x, body.min.y, body.size.w, body.size.h)
        };
        assert_eq!(h_pri_min, 0.0, "case: {label} header pri_min");
        assert_eq!(h_pri_size, 320.0, "case: {label} header pri_size");
        assert_eq!(h_sec_size, 40.0, "case: {label} header sec_size");
        assert_eq!(b_pri_min, 110.0, "case: {label} body pri_min");
        assert_eq!(b_sec_min, 50.0, "case: {label} body sec_min");
        assert_eq!(b_pri_size, 100.0, "case: {label} body pri_size");
        assert_eq!(b_sec_size, 40.0, "case: {label} body sec_size");
    }
}

#[test]
fn grid_hug_grid_collapses_fill_tracks() {
    let mut ui = Ui::for_test();
    let mut grid_node = None;
    ui.run_at(UVec2::new(400, 200), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                grid_node = Some(
                    Grid::new()
                        .id(WidgetId::from_hash("hug-grid"))
                        .cols([Track::fixed(80.0), Track::fill()])
                        .rows([Track::fixed(40.0)])
                        .size((Sizing::HUG, Sizing::HUG))
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
                        .node(),
                );
            });
    });
    let r = ui.layout[Layer::Main].rect[grid_node.unwrap().idx()];
    assert_eq!(r.size.w, 80.0, "hug grid collapses Fill col to 0");
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn grid_cell_alignment_override_pins_child_to_corner() {
    use crate::layout::types::{align::Align, align::HAlign, align::VAlign};

    let mut ui = Ui::for_test();
    let mut root = None;
    ui.run_at(UVec2::new(200, 200), |ui| {
        root = Some(
            Grid::new()
                .auto_id()
                .cols([Track::fixed(100.0)])
                .rows([Track::fixed(100.0)])
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("pinned"))
                        .grid_cell((0, 0))
                        .size((20.0, 20.0))
                        .align(Align::new(HAlign::Right, VAlign::Bottom))
                        .show(ui);
                })
                .node(),
        );
    });
    let r = child_rects(&ui, root.unwrap())[0];
    assert_eq!(r.size.w, 20.0);
    assert_eq!(r.size.h, 20.0);
    assert_eq!(r.min.x, 80.0);
    assert_eq!(r.min.y, 80.0);
}

/// Pins implicit contract: `Fixed`/`Hug` resolved, `Fill` unresolved so
/// cells see `INF` (WPF intrinsic trick that defers Fill until arrange).
#[test]
fn resolve_axis_marks_fixed_and_hug_resolved_but_leaves_fill_unresolved() {
    let tracks: Rc<[Track]> = Rc::from([Track::fixed(50.0), Track::hug(), Track::fill()]);
    let mut a = AxisScratch::default();
    a.reset(tracks);
    let hug_min = [0.0, 10.0, 0.0];
    let hug_max = [0.0, 30.0, 0.0];

    resolve_axis(&mut a, &hug_min, &hug_max, 200.0, 0.0, Sizing::HUG);

    assert!(
        a.resolved.contains(0) && a.resolved.contains(1) && !a.resolved.contains(2),
        "Fill cols must stay unresolved so `sum_spanned_known` returns INF for them"
    );
}

/// Pin: 2-D span (row + col) covers the rectangular union with gaps.
#[test]
fn grid_cell_with_2d_span_covers_track_union_with_gaps() {
    let mut ui = Ui::for_test();
    let mut root = None;
    // 3×3 of fixed-50 cells with gap=10. 2×2 cell at (0,0): w/h = 110.
    ui.run_at(UVec2::new(400, 400), |ui| {
        root = Some(
            Grid::new()
                .auto_id()
                .cols([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
                .rows([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
                .gap(10.0)
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("big"))
                        .grid_cell((0, 0))
                        .grid_span((2, 2))
                        .show(ui);
                    Frame::new()
                        .id(WidgetId::from_hash("corner"))
                        .grid_cell((2, 2))
                        .show(ui);
                })
                .node(),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    let big = kids[0];
    let corner = kids[1];

    assert_eq!((big.min.x, big.min.y), (0.0, 0.0));
    assert_eq!((big.size.w, big.size.h), (110.0, 110.0));
    assert_eq!((corner.min.x, corner.min.y), (120.0, 120.0));
    assert_eq!((corner.size.w, corner.size.h), (50.0, 50.0));
}

/// Pin: empty grid (zero rows or zero cols) measures + arranges to zero
/// without panicking; child rects are zeroed at parent anchor.
#[test]
fn grid_empty_dim_measures_to_zero_and_zeros_children() {
    let mut ui = Ui::for_test();
    let mut grid_node = None;
    let mut ghost_node = None;
    let empty: Rc<[Track]> = Rc::from([] as [Track; 0]);
    ui.run_at(UVec2::new(400, 400), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                grid_node = Some(
                    Grid::new()
                        .id(WidgetId::from_hash("empty-grid"))
                        .cols([Track::fixed(50.0)])
                        .rows(empty.clone())
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui, |ui| {
                            ghost_node = Some(
                                Frame::new()
                                    .id(WidgetId::from_hash("ghost"))
                                    .size((20.0, 20.0))
                                    .show(ui)
                                    .node(),
                            );
                        })
                        .node(),
                );
            });
    });
    let r = ui.layout[Layer::Main].rect[grid_node.unwrap().idx()];
    assert_eq!(r.size.w, 0.0);
    assert_eq!(r.size.h, 0.0);

    let ghost = ui.layout[Layer::Main].rect[ghost_node.unwrap().idx()];
    assert_eq!(ghost.size.w, 0.0);
    assert_eq!(ghost.size.h, 0.0);
}

/// Pin: each Hug row resolves to its own cells' max desired height,
/// independent of other rows.
#[test]
fn grid_multi_row_hug_heights_resolve_independently() {
    let mut ui = Ui::for_test();
    let mut grid_node = None;
    let mut kids = Vec::new();
    ui.run_at(UVec2::new(400, 400), |ui| {
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
                        .node(),
                );
            });
    });
    assert_eq!(ui.layout[Layer::Main].rect[kids[0].idx()].size.h, 10.0);
    assert_eq!(ui.layout[Layer::Main].rect[kids[1].idx()].size.h, 80.0);
    assert_eq!(ui.layout[Layer::Main].rect[kids[2].idx()].size.h, 30.0);
    assert_eq!(
        ui.layout[Layer::Main].rect[grid_node.unwrap().idx()].size.h,
        120.0
    );
}
