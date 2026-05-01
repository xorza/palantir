use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Rect, Sizing, Track};
use crate::widgets::{Button, Frame, Grid, Panel};

#[test]
fn grid_fixed_and_fill_columns_split_remainder() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = Grid::new()
        .cols([Track::fixed(120.0), Track::fill()])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::with_id("left").grid_cell((0, 0)).show(ui);
            Frame::with_id("right").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.layout(Rect::new(0.0, 0.0, 400.0, 200.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let left = ui.rect(kids[0]);
    let right = ui.rect(kids[1]);
    assert_eq!(left.size.w, 120.0);
    assert_eq!(left.min.x, 0.0);
    assert_eq!(right.size.w, 280.0);
    assert_eq!(right.min.x, 120.0);
    assert_eq!(left.size.h, 200.0);
    assert_eq!(right.size.h, 200.0);
}

#[test]
fn grid_hug_column_takes_max_span1_child_intrinsic() {
    let mut ui = Ui::new();
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 400.0, 200.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let short_btn = ui.rect(kids[0]);
    let long_btn = ui.rect(kids[1]);
    let body = ui.rect(kids[2]);
    // Hug col = max(16, 56) = 56 → x boundary at 56.
    assert_eq!(body.min.x, 56.0);
    assert_eq!(body.size.w, 344.0);
    assert_eq!(short_btn.min.x, 0.0);
    assert_eq!(long_btn.min.x, 0.0);
}

#[test]
fn grid_fill_weights_split_remainder_proportionally() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = Grid::new()
        .cols([Track::fill_weight(1.0), Track::fill_weight(3.0)])
        .rows([Track::fill()])
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::with_id("a").grid_cell((0, 0)).show(ui);
            Frame::with_id("b").grid_cell((0, 1)).show(ui);
        })
        .node;
    ui.layout(Rect::new(0.0, 0.0, 400.0, 100.0));
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.rect(kids[0]).size.w, 100.0);
    assert_eq!(ui.rect(kids[1]).size.w, 300.0);
}

#[test]
fn grid_fill_min_clamp_steals_from_other_stars() {
    let mut ui = Ui::new();
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 400.0, 100.0));
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.rect(kids[0]).size.w, 200.0);
    assert_eq!(ui.rect(kids[1]).size.w, 200.0);
}

#[test]
fn grid_fill_max_clamp_donates_to_other_stars() {
    let mut ui = Ui::new();
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 400.0, 100.0));
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.rect(kids[0]).size.w, 150.0);
    assert_eq!(ui.rect(kids[1]).size.w, 250.0);
}

#[test]
fn grid_col_span_covers_multiple_columns_with_gap() {
    let mut ui = Ui::new();
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 400.0, 200.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let header = ui.rect(kids[0]);
    let body = ui.rect(kids[1]);
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
    let mut ui = Ui::new();
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 400.0, 200.0));
    let r = ui.rect(grid_node.unwrap());
    assert_eq!(r.size.w, 80.0, "hug grid collapses Fill col to 0");
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn grid_row_span_covers_multiple_rows_with_gap() {
    // Mirror image of `grid_col_span_covers_multiple_columns_with_gap` — same
    // arithmetic, axes swapped. Pins that row-span and col-span share the
    // same code path.
    let mut ui = Ui::new();
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 200.0, 400.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let sidebar = ui.rect(kids[0]);
    let body = ui.rect(kids[1]);
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
    use crate::primitives::{Align, HAlign, VAlign};

    let mut ui = Ui::new();
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    let kids: Vec<_> = ui.tree.children(root).collect();
    let r = ui.rect(kids[0]);
    assert_eq!(r.size.w, 20.0);
    assert_eq!(r.size.h, 20.0);
    assert_eq!(r.min.x, 80.0);
    assert_eq!(r.min.y, 80.0);
}
