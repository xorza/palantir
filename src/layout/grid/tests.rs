use super::{AxisScratch, resolve_axis};
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::test_support::ui_at;
use crate::tree::element::Configure;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel};
use glam::UVec2;
use std::rc::Rc;

#[test]
fn grid_fixed_and_fill_columns_split_remainder() {
    let mut ui = ui_at(UVec2::new(400, 200));
    let root = Grid::new()
        .cols([Track::fixed(120.0), Track::fill()])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::with_id("left").grid_cell((0, 0)).show(ui);
            Frame::with_id("right").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let left = ui.layout_engine.result.rect(kids[0]);
    let right = ui.layout_engine.result.rect(kids[1]);
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
    // Hug col 0: max(label widths). Buttons measure label text at 8px/char × 16h.
    let root = Grid::new()
        .cols([Track::hug(), Track::fill()])
        .rows([Track::hug(), Track::hug()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Button::with_id("short")
                .label("ok")
                .grid_cell((0, 0))
                .show(ui); // 16w
            Button::with_id("long")
                .label("hello!!")
                .grid_cell((1, 0))
                .show(ui); // 56w
            Frame::with_id("body")
                .grid_cell((0, 1))
                .grid_span((2, 1))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let short_btn = ui.layout_engine.result.rect(kids[0]);
    let long_btn = ui.layout_engine.result.rect(kids[1]);
    let body = ui.layout_engine.result.rect(kids[2]);
    // Hug col = max(16, 56) = 56 → x boundary at 56.
    assert_eq!(body.min.x, 56.0);
    assert_eq!(body.size.w, 344.0);
    assert_eq!(short_btn.min.x, 0.0);
    assert_eq!(long_btn.min.x, 0.0);
}

#[test]
fn grid_fill_weights_split_remainder_proportionally() {
    let mut ui = ui_at(UVec2::new(400, 100));
    let root = Grid::new()
        .cols([Track::fill_weight(1.0), Track::fill_weight(3.0)])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::with_id("a").grid_cell((0, 0)).show(ui);
            Frame::with_id("b").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.end_frame();
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.layout_engine.result.rect(kids[0]).size.w, 100.0);
    assert_eq!(ui.layout_engine.result.rect(kids[1]).size.w, 300.0);
}

#[test]
fn grid_fill_min_clamp_steals_from_other_stars() {
    let mut ui = ui_at(UVec2::new(400, 100));
    // Fill col 0 wants 100 (1/4 of 400), but min=200 → it clamps to 200,
    // remaining 200 distributes to col 1 (weight 3 → 200).
    let root = Grid::new()
        .cols([Track::fill_weight(1.0).min(200.0), Track::fill_weight(3.0)])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::with_id("a").grid_cell((0, 0)).show(ui);
            Frame::with_id("b").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.end_frame();
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.layout_engine.result.rect(kids[0]).size.w, 200.0);
    assert_eq!(ui.layout_engine.result.rect(kids[1]).size.w, 200.0);
}

#[test]
fn grid_fill_max_clamp_donates_to_other_stars() {
    let mut ui = ui_at(UVec2::new(400, 100));
    // Fill col 0 wants 300 (3/4 of 400) but max=150 → clamps; col 1 takes 250.
    let root = Grid::new()
        .cols([Track::fill_weight(3.0).max(150.0), Track::fill_weight(1.0)])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::with_id("a").grid_cell((0, 0)).show(ui);
            Frame::with_id("b").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.end_frame();
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.layout_engine.result.rect(kids[0]).size.w, 150.0);
    assert_eq!(ui.layout_engine.result.rect(kids[1]).size.w, 250.0);
}

#[test]
fn grid_col_span_covers_multiple_columns_with_gap() {
    let mut ui = ui_at(UVec2::new(400, 200));
    // 3 fixed cols of 100 with gap 10 → header spanning all = 100+10+100+10+100 = 320.
    let root = Grid::new()
        .cols([
            Track::fixed(100.0),
            Track::fixed(100.0),
            Track::fixed(100.0),
        ])
        .rows([Track::fixed(40.0), Track::fixed(40.0)])
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::with_id("header")
                .grid_cell((0, 0))
                .grid_span((1, 3))
                .show(ui);
            Frame::with_id("body").grid_cell((1, 1)).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let header = ui.layout_engine.result.rect(kids[0]);
    let body = ui.layout_engine.result.rect(kids[1]);
    assert_eq!(header.min.x, 0.0);
    assert_eq!(header.size.w, 320.0);
    assert_eq!(header.size.h, 40.0);
    assert_eq!(body.min.x, 110.0);
    assert_eq!(body.min.y, 50.0);
    assert_eq!(body.size.w, 100.0);
    assert_eq!(body.size.h, 40.0);
}

#[test]
fn grid_hug_grid_collapses_fill_tracks() {
    let mut ui = ui_at(UVec2::new(400, 200));
    // Wrap in HStack so the Hug grid's measured size is honored — root in
    // `ui.layout` is forced to the surface size regardless of Sizing.
    let mut grid_node = None;
    let _root = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            grid_node = Some(
                Grid::with_id("hug-grid")
                    .cols([Track::fixed(80.0), Track::fill()])
                    .rows([Track::fixed(40.0)])
                    .size((Sizing::Hug, Sizing::Hug))
                    .show(ui, |ui| {
                        Frame::with_id("a").grid_cell((0, 0)).show(ui);
                        Frame::with_id("b").grid_cell((0, 1)).show(ui);
                    })
                    .node,
            );
        })
        .node;
    ui.end_frame();
    let r = ui.layout_engine.result.rect(grid_node.unwrap());
    assert_eq!(r.size.w, 80.0, "hug grid collapses Fill col to 0");
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn grid_row_span_covers_multiple_rows_with_gap() {
    // Mirror image of `grid_col_span_covers_multiple_columns_with_gap` — same
    // arithmetic, axes swapped. Pins that row-span and col-span share the
    // same code path.
    let mut ui = ui_at(UVec2::new(200, 400));
    let root = Grid::new()
        .rows([
            Track::fixed(100.0),
            Track::fixed(100.0),
            Track::fixed(100.0),
        ])
        .cols([Track::fixed(40.0), Track::fixed(40.0)])
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::with_id("sidebar")
                .grid_cell((0, 0))
                .grid_span((3, 1))
                .show(ui);
            Frame::with_id("body").grid_cell((1, 1)).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let sidebar = ui.layout_engine.result.rect(kids[0]);
    let body = ui.layout_engine.result.rect(kids[1]);
    assert_eq!(sidebar.min.y, 0.0);
    assert_eq!(sidebar.size.w, 40.0);
    assert_eq!(sidebar.size.h, 320.0);
    assert_eq!(body.min.x, 50.0);
    assert_eq!(body.min.y, 110.0);
    assert_eq!(body.size.w, 40.0);
    assert_eq!(body.size.h, 100.0);
}

#[test]
fn grid_cell_alignment_override_pins_child_to_corner() {
    // Default grid placement is auto-stretch (WPF cell behaviour). A child
    // with an explicit non-stretch align should size to its own intrinsic and
    // park at the requested corner of the cell.
    use crate::layout::types::{align::Align, align::HAlign, align::VAlign};

    let mut ui = ui_at(UVec2::new(200, 200));
    let root = Grid::new()
        .cols([Track::fixed(100.0)])
        .rows([Track::fixed(100.0)])
        .show(&mut ui, |ui| {
            Frame::with_id("pinned")
                .grid_cell((0, 0))
                .size((20.0, 20.0))
                .align(Align::new(HAlign::Right, VAlign::Bottom))
                .show(ui);
        })
        .node;
    ui.end_frame();
    let kids: Vec<_> = ui.tree.children(root).collect();
    let r = ui.layout_engine.result.rect(kids[0]);
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

    assert_eq!(
        a.resolved.as_slice(),
        &[true, true, false],
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
        .cols([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
        .rows([Track::fixed(50.0), Track::fixed(50.0), Track::fixed(50.0)])
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::with_id("big")
                .grid_cell((0, 0))
                .grid_span((2, 2))
                .show(ui);
            Frame::with_id("corner").grid_cell((2, 2)).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let big = ui.layout_engine.result.rect(kids[0]);
    let corner = ui.layout_engine.result.rect(kids[1]);

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
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            grid_node = Some(
                Grid::with_id("empty-grid")
                    .cols([Track::fixed(50.0)])
                    .rows(empty)
                    .size((Sizing::Hug, Sizing::Hug))
                    .show(ui, |ui| {
                        ghost_node = Some(Frame::with_id("ghost").size((20.0, 20.0)).show(ui).node);
                    })
                    .node,
            );
        });
    ui.end_frame();

    let r = ui.layout_engine.result.rect(grid_node.unwrap());
    assert_eq!(r.size.w, 0.0);
    assert_eq!(r.size.h, 0.0);

    let ghost = ui.layout_engine.result.rect(ghost_node.unwrap());
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
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            grid_node = Some(
                Grid::with_id("multi-row")
                    .cols([Track::fixed(50.0)])
                    .rows([Track::hug(), Track::hug(), Track::hug()])
                    .size((Sizing::Hug, Sizing::Hug))
                    .show(ui, |ui| {
                        kids.push(
                            Frame::with_id("short")
                                .size((50.0, 10.0))
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        kids.push(
                            Frame::with_id("tall")
                                .size((50.0, 80.0))
                                .grid_cell((1, 0))
                                .show(ui)
                                .node,
                        );
                        kids.push(
                            Frame::with_id("med")
                                .size((50.0, 30.0))
                                .grid_cell((2, 0))
                                .show(ui)
                                .node,
                        );
                    })
                    .node,
            );
        });
    ui.end_frame();

    assert_eq!(ui.layout_engine.result.rect(kids[0]).size.h, 10.0);
    assert_eq!(ui.layout_engine.result.rect(kids[1]).size.h, 80.0);
    assert_eq!(ui.layout_engine.result.rect(kids[2]).size.h, 30.0);
    // Grid hugs to sum + (n-1)*0 (no row gap set) = 120.
    assert_eq!(
        ui.layout_engine.result.rect(grid_node.unwrap()).size.h,
        120.0
    );
}
