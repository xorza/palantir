use super::{AxisScratch, resolve_axis};
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::{Layer, NodeId};
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::rect::Rect;
use crate::support::internals::ResponseNodeExt;
use crate::support::testing::new_ui;
use crate::support::testing::run_at;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel};
use glam::UVec2;
use std::rc::Rc;

fn child_rects(ui: &Ui, root: NodeId) -> Vec<Rect> {
    ui.forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| ui.layout[Layer::Main].rect[c.id.index()])
        .collect()
}

#[test]
fn grid_fixed_and_fill_columns_split_remainder() {
    let mut ui = new_ui();
    let mut root = None;
    run_at(&mut ui, UVec2::new(400, 200), |ui| {
        root = Some(
            Grid::new()
                .auto_id()
                .cols([Track::fixed(120.0), Track::fill()])
                .rows([Track::fill()])
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Frame::new().id_salt("left").grid_cell((0, 0)).show(ui);
                    Frame::new().id_salt("right").grid_cell((0, 1)).show(ui);
                })
                .node(ui),
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
    let mut ui = new_ui();
    let mut root = None;
    // Hug col 0: max(button widths). Buttons measure label at 8px/char +
    // default padding 24 → label_w + 24.
    run_at(&mut ui, UVec2::new(400, 200), |ui| {
        root = Some(
            Grid::new()
                .auto_id()
                .cols([Track::hug(), Track::fill()])
                .rows([Track::hug(), Track::hug()])
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Button::new()
                        .id_salt("short")
                        .label("ok")
                        .grid_cell((0, 0))
                        .show(ui);
                    Button::new()
                        .id_salt("long")
                        .label("hello!!")
                        .grid_cell((1, 0))
                        .show(ui);
                    Frame::new()
                        .id_salt("body")
                        .grid_cell((0, 1))
                        .grid_span((2, 1))
                        .show(ui);
                })
                .node(ui),
        );
    });
    let kids = child_rects(&ui, root.unwrap());
    let short_btn = kids[0];
    let long_btn = kids[1];
    let body = kids[2];
    assert_eq!(body.min.x, 80.0);
    assert_eq!(body.size.w, 320.0);
    assert_eq!(short_btn.min.x, 0.0);
    assert_eq!(long_btn.min.x, 0.0);
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
    ];
    for (label, c0, c1, want0, want1) in cases {
        let mut ui = new_ui();
        let mut root = None;
        run_at(&mut ui, UVec2::new(400, 100), |ui| {
            root = Some(
                Grid::new()
                    .auto_id()
                    .cols([*c0, *c1])
                    .rows([Track::fill()])
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Frame::new().id_salt("a").grid_cell((0, 0)).show(ui);
                        Frame::new().id_salt("b").grid_cell((0, 1)).show(ui);
                    })
                    .node(ui),
            );
        });
        let kids = child_rects(&ui, root.unwrap());
        assert_eq!(kids[0].size.w, *want0, "case: {label} col0");
        assert_eq!(kids[1].size.w, *want1, "case: {label} col1");
    }
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
        let mut ui = new_ui();
        let mut root = None;
        run_at(&mut ui, surface, |ui| {
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
                            .id_salt("header")
                            .grid_cell((0, 0))
                            .grid_span(span)
                            .show(ui);
                        Frame::new().id_salt("body").grid_cell((1, 1)).show(ui);
                    })
                    .node(ui),
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
    let mut ui = new_ui();
    let mut grid_node = None;
    run_at(&mut ui, UVec2::new(400, 200), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                grid_node = Some(
                    Grid::new()
                        .id_salt("hug-grid")
                        .cols([Track::fixed(80.0), Track::fill()])
                        .rows([Track::fixed(40.0)])
                        .size((Sizing::Hug, Sizing::Hug))
                        .show(ui, |ui| {
                            Frame::new().id_salt("a").grid_cell((0, 0)).show(ui);
                            Frame::new().id_salt("b").grid_cell((0, 1)).show(ui);
                        })
                        .node(ui),
                );
            });
    });
    let r = ui.layout[Layer::Main].rect[grid_node.unwrap().index()];
    assert_eq!(r.size.w, 80.0, "hug grid collapses Fill col to 0");
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn grid_cell_alignment_override_pins_child_to_corner() {
    use crate::layout::types::{align::Align, align::HAlign, align::VAlign};

    let mut ui = new_ui();
    let mut root = None;
    run_at(&mut ui, UVec2::new(200, 200), |ui| {
        root = Some(
            Grid::new()
                .auto_id()
                .cols([Track::fixed(100.0)])
                .rows([Track::fixed(100.0)])
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("pinned")
                        .grid_cell((0, 0))
                        .size((20.0, 20.0))
                        .align(Align::new(HAlign::Right, VAlign::Bottom))
                        .show(ui);
                })
                .node(ui),
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

    resolve_axis(&mut a, &hug_min, &hug_max, 200.0, 0.0, Sizing::Hug);

    assert!(
        a.resolved.contains(0) && a.resolved.contains(1) && !a.resolved.contains(2),
        "Fill cols must stay unresolved so `sum_spanned_known` returns INF for them"
    );
}

/// Pin: 2-D span (row + col) covers the rectangular union with gaps.
#[test]
fn grid_cell_with_2d_span_covers_track_union_with_gaps() {
    let mut ui = new_ui();
    let mut root = None;
    // 3×3 of fixed-50 cells with gap=10. 2×2 cell at (0,0): w/h = 110.
    run_at(&mut ui, UVec2::new(400, 400), |ui| {
        root = Some(
            Grid::new()
                .auto_id()
                .cols([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
                .rows([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
                .gap(10.0)
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("big")
                        .grid_cell((0, 0))
                        .grid_span((2, 2))
                        .show(ui);
                    Frame::new().id_salt("corner").grid_cell((2, 2)).show(ui);
                })
                .node(ui),
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
    let mut ui = new_ui();
    let mut grid_node = None;
    let mut ghost_node = None;
    let empty: Rc<[Track]> = Rc::from([] as [Track; 0]);
    run_at(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                grid_node = Some(
                    Grid::new()
                        .id_salt("empty-grid")
                        .cols([Track::fixed(50.0)])
                        .rows(empty.clone())
                        .size((Sizing::Hug, Sizing::Hug))
                        .show(ui, |ui| {
                            ghost_node = Some(
                                Frame::new()
                                    .id_salt("ghost")
                                    .size((20.0, 20.0))
                                    .show(ui)
                                    .node(ui),
                            );
                        })
                        .node(ui),
                );
            });
    });
    let r = ui.layout[Layer::Main].rect[grid_node.unwrap().index()];
    assert_eq!(r.size.w, 0.0);
    assert_eq!(r.size.h, 0.0);

    let ghost = ui.layout[Layer::Main].rect[ghost_node.unwrap().index()];
    assert_eq!(ghost.size.w, 0.0);
    assert_eq!(ghost.size.h, 0.0);
}

/// Pin: each Hug row resolves to its own cells' max desired height,
/// independent of other rows.
#[test]
fn grid_multi_row_hug_heights_resolve_independently() {
    let mut ui = new_ui();
    let mut grid_node = None;
    let mut kids = Vec::new();
    run_at(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                grid_node = Some(
                    Grid::new()
                        .id_salt("multi-row")
                        .cols([Track::fixed(50.0)])
                        .rows([Track::hug(), Track::hug(), Track::hug()])
                        .size((Sizing::Hug, Sizing::Hug))
                        .show(ui, |ui| {
                            kids.push(
                                Frame::new()
                                    .id_salt("short")
                                    .size((50.0, 10.0))
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(ui),
                            );
                            kids.push(
                                Frame::new()
                                    .id_salt("tall")
                                    .size((50.0, 80.0))
                                    .grid_cell((1, 0))
                                    .show(ui)
                                    .node(ui),
                            );
                            kids.push(
                                Frame::new()
                                    .id_salt("med")
                                    .size((50.0, 30.0))
                                    .grid_cell((2, 0))
                                    .show(ui)
                                    .node(ui),
                            );
                        })
                        .node(ui),
                );
            });
    });
    assert_eq!(ui.layout[Layer::Main].rect[kids[0].index()].size.h, 10.0);
    assert_eq!(ui.layout[Layer::Main].rect[kids[1].index()].size.h, 80.0);
    assert_eq!(ui.layout[Layer::Main].rect[kids[2].index()].size.h, 30.0);
    assert_eq!(
        ui.layout[Layer::Main].rect[grid_node.unwrap().index()]
            .size
            .h,
        120.0
    );
}
