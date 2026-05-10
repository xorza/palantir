use super::{AxisScratch, resolve_axis};
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::support::testing::ui_at;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel};
use glam::UVec2;
use std::rc::Rc;

#[test]
fn grid_fixed_and_fill_columns_split_remainder() {
    let mut ui = ui_at(UVec2::new(400, 200));
    let root = Grid::new()
        .auto_id()
        .cols([Track::fixed(120.0), Track::fill()])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new().id_salt("left").grid_cell((0, 0)).show(ui);
            Frame::new().id_salt("right").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    let left = ui.layout.result[Layer::Main].rect[kids[0].index()];
    let right = ui.layout.result[Layer::Main].rect[kids[1].index()];
    assert_eq!(left.size.w, 120.0);
    assert_eq!(left.min.x, 0.0);
    assert_eq!(right.size.w, 280.0);
    assert_eq!(right.min.x, 120.0);
    assert_eq!(left.size.h, 200.0);
    assert_eq!(right.size.h, 200.0);
}

#[test]
fn grid_hug_column_takes_max_span1_child_intrinsic() {
    let mut ui = ui_at(UVec2::new(400, 200));
    // Hug col 0: max(button widths). Buttons measure label at 8px/char,
    // plus default `ButtonTheme.padding = Spacing::xy(12.0, 6.0)`, so
    // the button width is `label_w + 24`.
    let root = Grid::new()
        .auto_id()
        .cols([Track::hug(), Track::fill()])
        .rows([Track::hug(), Track::hug()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Button::new()
                .id_salt("short")
                .label("ok")
                .grid_cell((0, 0))
                .show(ui); // 16 + 24 = 40w
            Button::new()
                .id_salt("long")
                .label("hello!!")
                .grid_cell((1, 0))
                .show(ui); // 56 + 24 = 80w
            Frame::new()
                .id_salt("body")
                .grid_cell((0, 1))
                .grid_span((2, 1))
                .show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    let short_btn = ui.layout.result[Layer::Main].rect[kids[0].index()];
    let long_btn = ui.layout.result[Layer::Main].rect[kids[1].index()];
    let body = ui.layout.result[Layer::Main].rect[kids[2].index()];
    // Hug col = max(40, 80) = 80 → x boundary at 80.
    assert_eq!(body.min.x, 80.0);
    assert_eq!(body.size.w, 320.0);
    assert_eq!(short_btn.min.x, 0.0);
    assert_eq!(long_btn.min.x, 0.0);
}

#[test]
fn grid_fill_weights_split_remainder_proportionally() {
    let mut ui = ui_at(UVec2::new(400, 100));
    let root = Grid::new()
        .auto_id()
        .cols([Track::fill_weight(1.0), Track::fill_weight(3.0)])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new().id_salt("a").grid_cell((0, 0)).show(ui);
            Frame::new().id_salt("b").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[0].index()].size.w,
        100.0
    );
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[1].index()].size.w,
        300.0
    );
}

#[test]
fn grid_fill_min_clamp_steals_from_other_stars() {
    let mut ui = ui_at(UVec2::new(400, 100));
    // Fill col 0 wants 100 (1/4 of 400), but min=200 → it clamps to 200,
    // remaining 200 distributes to col 1 (weight 3 → 200).
    let root = Grid::new()
        .auto_id()
        .cols([Track::fill_weight(1.0).min(200.0), Track::fill_weight(3.0)])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new().id_salt("a").grid_cell((0, 0)).show(ui);
            Frame::new().id_salt("b").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[0].index()].size.w,
        200.0
    );
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[1].index()].size.w,
        200.0
    );
}

#[test]
fn grid_fill_max_clamp_donates_to_other_stars() {
    let mut ui = ui_at(UVec2::new(400, 100));
    // Fill col 0 wants 300 (3/4 of 400) but max=150 → clamps; col 1 takes 250.
    let root = Grid::new()
        .auto_id()
        .cols([Track::fill_weight(3.0).max(150.0), Track::fill_weight(1.0)])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new().id_salt("a").grid_cell((0, 0)).show(ui);
            Frame::new().id_salt("b").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[0].index()].size.w,
        150.0
    );
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[1].index()].size.w,
        250.0
    );
}

#[test]
fn grid_span_covers_multiple_tracks_with_gap() {
    // 3 fixed primary tracks of 100 with gap 10 → spanning all = 320.
    // Body sits in track (1,1) → 110 offset on primary, 50 on secondary.
    // Pins col-span and row-span share the same code path (mirror axes).
    let cases: &[(&str, bool)] = &[("col_span", false), ("row_span", true)];
    for (label, swap) in cases {
        let surface = if *swap {
            UVec2::new(200, 400)
        } else {
            UVec2::new(400, 200)
        };
        let mut ui = ui_at(surface);
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
        let root = g
            .gap(10.0)
            .show(&mut ui, |ui| {
                Frame::new()
                    .id_salt("header")
                    .grid_cell((0, 0))
                    .grid_span(span)
                    .show(ui);
                Frame::new().id_salt("body").grid_cell((1, 1)).show(ui);
            })
            .node;
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        let kids: Vec<_> = ui
            .forest
            .tree(Layer::Main)
            .children(root)
            .map(|c| c.id)
            .collect();
        let header = ui.layout.result[Layer::Main].rect[kids[0].index()];
        let body = ui.layout.result[Layer::Main].rect[kids[1].index()];
        // (primary, secondary) → (x, y) when not swapped; (y, x) when swapped.
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
    let mut ui = ui_at(UVec2::new(400, 200));
    // Wrap in HStack so the Hug grid's measured size is honored — root in
    // `ui.layout` is forced to the surface size regardless of Sizing.
    let mut grid_node = None;
    let _root = Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
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
                    .node,
            );
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let r = ui.layout.result[Layer::Main].rect[grid_node.unwrap().index()];
    assert_eq!(r.size.w, 80.0, "hug grid collapses Fill col to 0");
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn grid_cell_alignment_override_pins_child_to_corner() {
    // Default grid placement is auto-stretch (WPF cell behaviour). A child
    // with an explicit non-stretch align should size to its own intrinsic and
    // park at the requested corner of the cell.
    use crate::layout::types::{align::Align, align::HAlign, align::VAlign};

    let mut ui = ui_at(UVec2::new(200, 200));
    let root = Grid::new()
        .auto_id()
        .cols([Track::fixed(100.0)])
        .rows([Track::fixed(100.0)])
        .show(&mut ui, |ui| {
            Frame::new()
                .id_salt("pinned")
                .grid_cell((0, 0))
                .size((20.0, 20.0))
                .align(Align::new(HAlign::Right, VAlign::Bottom))
                .show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    let r = ui.layout.result[Layer::Main].rect[kids[0].index()];
    assert_eq!(r.size.w, 20.0);
    assert_eq!(r.size.h, 20.0);
    assert_eq!(r.min.x, 80.0);
    assert_eq!(r.min.y, 80.0);
}

/// Locks the implicit contract `sum_spanned_known` depends on: after
/// `resolve_axis` runs, `Fixed` and `Hug` tracks are flagged resolved
/// while `Fill` tracks stay unresolved so cells in Fill cols see
/// `INFINITY` as their available width (the WPF intrinsic trick that
/// lets Fill widths only finalize at arrange time). Today this is an
/// emergent property of phase ordering; this test pins it.
#[test]
fn resolve_axis_marks_fixed_and_hug_resolved_but_leaves_fill_unresolved() {
    let tracks: Rc<[Track]> = Rc::from([Track::fixed(50.0), Track::hug(), Track::fill()]);
    let mut a = AxisScratch::default();
    a.reset(tracks);
    let hug_min = [0.0, 10.0, 0.0];
    let hug_max = [0.0, 30.0, 0.0];

    // Pass `Sizing::Hug` for the grid's own axis sizing so Phase 4
    // skips the Fill-resolved commit — that's the contract this test
    // pins (cells in Fill cols see INF until arrange).
    resolve_axis(&mut a, &hug_min, &hug_max, 200.0, 0.0, Sizing::Hug);

    assert!(
        a.resolved.contains(0) && a.resolved.contains(1) && !a.resolved.contains(2),
        "Fill cols must stay unresolved so `sum_spanned_known` returns INF for them"
    );
}

/// Pin: a cell with both `row_span > 1` and `col_span > 1` covers the
/// rectangular union of those tracks (gaps between spanned tracks
/// included). Today's tests cover row_span and col_span separately;
/// this exercises the 2-D case which goes through `span_size` on both
/// axes plus `record_hug`'s `span != 1` skip on both axes.
#[test]
fn grid_cell_with_2d_span_covers_track_union_with_gaps() {
    let mut ui = ui_at(UVec2::new(400, 400));
    // 3×3 of fixed-50 cells with gap=10. A 2×2 cell starting at (0,0)
    // covers rows 0-1 and cols 0-1: w = 50+10+50 = 110, h = same.
    let root = Grid::new()
        .auto_id()
        .cols([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
        .rows([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::new()
                .id_salt("big")
                .grid_cell((0, 0))
                .grid_span((2, 2))
                .show(ui);
            Frame::new().id_salt("corner").grid_cell((2, 2)).show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    let big = ui.layout.result[Layer::Main].rect[kids[0].index()];
    let corner = ui.layout.result[Layer::Main].rect[kids[1].index()];

    assert_eq!((big.min.x, big.min.y), (0.0, 0.0));
    assert_eq!((big.size.w, big.size.h), (110.0, 110.0));
    // corner sits at row 2 col 2: x = 2*(50+10) = 120, y = 120.
    assert_eq!((corner.min.x, corner.min.y), (120.0, 120.0));
    assert_eq!((corner.size.w, corner.size.h), (50.0, 50.0));
}

/// Pin: an empty grid (zero rows or zero cols) measures + arranges
/// without panicking; its content size is `Size::ZERO` and any
/// child's rect is zeroed at the parent's anchor.
/// `grid::measure_inner` and `grid::arrange_inner` both have an
/// early-return shortcut for this case; without a test, removing the
/// shortcut would silently start panicking on track indexing or
/// producing garbage rects.
#[test]
fn grid_empty_dim_measures_to_zero_and_zeros_children() {
    let mut ui = ui_at(UVec2::new(400, 400));
    // Zero-row grid via explicit empty rows. Wrapped in HStack so the
    // Hug grid's measured (zero) size is honored — `ui.layout` forces
    // the root rect to the surface size regardless of Sizing.
    let empty: Rc<[Track]> = Rc::from([] as [Track; 0]);
    let mut grid_node = None;
    let mut ghost_node = None;
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            grid_node = Some(
                Grid::new()
                    .id_salt("empty-grid")
                    .cols([Track::fixed(50.0)])
                    .rows(empty)
                    .size((Sizing::Hug, Sizing::Hug))
                    .show(ui, |ui| {
                        ghost_node = Some(
                            Frame::new()
                                .id_salt("ghost")
                                .size((20.0, 20.0))
                                .show(ui)
                                .node,
                        );
                    })
                    .node,
            );
        });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let r = ui.layout.result[Layer::Main].rect[grid_node.unwrap().index()];
    assert_eq!(r.size.w, 0.0);
    assert_eq!(r.size.h, 0.0);

    let ghost = ui.layout.result[Layer::Main].rect[ghost_node.unwrap().index()];
    assert_eq!(ghost.size.w, 0.0);
    assert_eq!(ghost.size.h, 0.0);
}

/// Pin: each Hug row resolves to its own cells' max desired height,
/// independent of other rows. A taller cell in row 1 must not affect
/// row 0's height. Today tests cover single Hug rows; this catches a
/// bug where `record_hug` accidentally writes to the wrong row index.
#[test]
fn grid_multi_row_hug_heights_resolve_independently() {
    let mut ui = ui_at(UVec2::new(400, 400));
    // Wrap in HStack so the Hug-on-h grid's measured size is honored —
    // root forces the surface size regardless of Sizing.
    let mut grid_node = None;
    let mut kids = Vec::new();
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
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
                                .node,
                        );
                        kids.push(
                            Frame::new()
                                .id_salt("tall")
                                .size((50.0, 80.0))
                                .grid_cell((1, 0))
                                .show(ui)
                                .node,
                        );
                        kids.push(
                            Frame::new()
                                .id_salt("med")
                                .size((50.0, 30.0))
                                .grid_cell((2, 0))
                                .show(ui)
                                .node,
                        );
                    })
                    .node,
            );
        });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[0].index()].size.h,
        10.0
    );
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[1].index()].size.h,
        80.0
    );
    assert_eq!(
        ui.layout.result[Layer::Main].rect[kids[2].index()].size.h,
        30.0
    );
    // Grid hugs to sum + (n-1)*0 (no row gap set) = 120.
    assert_eq!(
        ui.layout.result[Layer::Main].rect[grid_node.unwrap().index()]
            .size
            .h,
        120.0
    );
}
