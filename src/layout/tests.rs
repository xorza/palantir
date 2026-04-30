use crate::element::Element;
use crate::primitives::{Align, Rect, Sizing, Track};
use crate::widgets::{Button, Frame, Grid, HStack, VStack};
use crate::{Ui, layout};

#[test]
fn hstack_arranges_two_buttons_side_by_side() {
    let mut ui = Ui::new();
    ui.begin_frame();

    let root = HStack::new()
        .show(&mut ui, |ui| {
            Button::new().label("Hi").show(ui);
            Button::new()
                .label("World")
                .size((100.0, Sizing::Hug))
                .show(ui);
        })
        .node;

    let surface = Rect::new(0.0, 0.0, 800.0, 600.0);
    layout::run(&mut ui.tree, root, surface);

    assert_eq!(ui.tree.node(root).rect, surface);

    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(kids.len(), 2);

    // "Hi" measures 2*8=16 wide, 16 tall via the placeholder text metrics.
    // Button has no default padding, so its desired size matches the label.
    let a = ui.tree.node(kids[0]).rect;
    assert_eq!(a.min.x, 0.0);
    assert_eq!(a.min.y, 0.0);
    assert_eq!(a.size.w, 16.0);
    assert_eq!(a.size.h, 16.0);

    let b = ui.tree.node(kids[1]).rect;
    assert_eq!(b.min.x, 16.0);
    assert_eq!(b.size.w, 100.0);
    assert_eq!(b.size.h, 16.0);
}

#[test]
fn vstack_with_fill_distributes_remainder() {
    let mut ui = Ui::new();
    ui.begin_frame();

    let root = VStack::new()
        .show(&mut ui, |ui| {
            Button::new().size((Sizing::Hug, 50.0)).show(ui);
            Button::new().size((Sizing::Hug, Sizing::FILL)).show(ui);
        })
        .node;

    let surface = Rect::new(0.0, 0.0, 200.0, 300.0);
    layout::run(&mut ui.tree, root, surface);

    let kids: Vec<_> = ui.tree.children(root).collect();
    let fixed = ui.tree.node(kids[0]).rect;
    let filler = ui.tree.node(kids[1]).rect;

    assert_eq!(fixed.size.h, 50.0);
    assert_eq!(filler.min.y, 50.0);
    assert_eq!(filler.size.h, 250.0);
}

#[test]
fn hstack_fill_weights_split_remainder_proportionally() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .show(&mut ui, |ui| {
            Frame::with_id("a")
                .size((Sizing::Fill(1.0), Sizing::Hug))
                .show(ui);
            Frame::with_id("b")
                .size((Sizing::Fill(3.0), Sizing::Hug))
                .show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.tree.node(kids[0]).rect;
    let b = ui.tree.node(kids[1]).rect;
    // 400 leftover / 4 weight = 100 per weight unit → a=100, b=300.
    assert_eq!(a.size.w, 100.0);
    assert_eq!(b.size.w, 300.0);
    assert_eq!(b.min.x, 100.0);
}

#[test]
fn hstack_equal_fill_siblings_are_equal_width_regardless_of_content() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .show(&mut ui, |ui| {
            Button::with_id("wide")
                .label("wide button")
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui);
            Button::with_id("narrow")
                .label("x")
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.tree.node(kids[0]).rect;
    let b = ui.tree.node(kids[1]).rect;
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 200.0);
    assert_eq!(a.min.x, 0.0);
    assert_eq!(b.min.x, 200.0);
}

#[test]
fn hstack_justify_center_centers_content_block() {
    use crate::primitives::Justify;
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .justify(Justify::Center)
        .show(&mut ui, |ui| {
            Frame::with_id("a").size(40.0).show(ui);
            Frame::with_id("b").size(40.0).show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Two 40-wide children, no gap → content width = 80. Leftover = 120,
    // half = 60 padding on the leading edge.
    assert_eq!(ui.tree.node(kids[0]).rect.min.x, 60.0);
    assert_eq!(ui.tree.node(kids[1]).rect.min.x, 100.0);
}

#[test]
fn hstack_justify_end_packs_to_trailing_edge() {
    use crate::primitives::Justify;
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .justify(Justify::End)
        .show(&mut ui, |ui| {
            Frame::with_id("a").size(40.0).show(ui);
            Frame::with_id("b").size(40.0).show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Last child ends at 200; 40 wide → starts at 160. First at 120.
    assert_eq!(ui.tree.node(kids[0]).rect.min.x, 120.0);
    assert_eq!(ui.tree.node(kids[1]).rect.min.x, 160.0);
}

#[test]
fn hstack_justify_space_between_distributes_leftover_between() {
    use crate::primitives::Justify;
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .justify(Justify::SpaceBetween)
        .show(&mut ui, |ui| {
            Frame::with_id("a").size(40.0).show(ui);
            Frame::with_id("b").size(40.0).show(ui);
            Frame::with_id("c").size(40.0).show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Leftover = 200 - 120 = 80, split into 2 gaps of 40.
    assert_eq!(ui.tree.node(kids[0]).rect.min.x, 0.0);
    assert_eq!(ui.tree.node(kids[1]).rect.min.x, 80.0);
    assert_eq!(ui.tree.node(kids[2]).rect.min.x, 160.0);
}

#[test]
fn hstack_justify_space_around_distributes_with_half_pads() {
    use crate::primitives::Justify;
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .justify(Justify::SpaceAround)
        .show(&mut ui, |ui| {
            Frame::with_id("a").size(40.0).show(ui);
            Frame::with_id("b").size(40.0).show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Leftover = 120, /count(2) = 60 per slot. Half (30) padding before first,
    // full 60 between, half (30) after last.
    assert_eq!(ui.tree.node(kids[0]).rect.min.x, 30.0);
    // 30 + 40 + 60 = 130
    assert_eq!(ui.tree.node(kids[1]).rect.min.x, 130.0);
}

#[test]
fn hstack_justify_is_noop_when_fill_child_consumes_leftover() {
    use crate::primitives::Justify;
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .justify(Justify::Center)
        .show(&mut ui, |ui| {
            Frame::with_id("a").size(40.0).show(ui);
            Frame::with_id("filler")
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui);
            Frame::with_id("c").size(40.0).show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Fill consumes leftover → first child still pinned to start.
    assert_eq!(ui.tree.node(kids[0]).rect.min.x, 0.0);
    assert_eq!(ui.tree.node(kids[1]).rect.min.x, 40.0);
    assert_eq!(ui.tree.node(kids[1]).rect.size.w, 120.0);
    assert_eq!(ui.tree.node(kids[2]).rect.min.x, 160.0);
}

#[test]
fn hstack_gap_inserts_space_between_children() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::with_id("a").size(40.0).show(ui);
            Frame::with_id("b").size(40.0).show(ui);
            Frame::with_id("c").size(40.0).show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.tree.node(kids[0]).rect.min.x, 0.0);
    assert_eq!(ui.tree.node(kids[1]).rect.min.x, 50.0);
    assert_eq!(ui.tree.node(kids[2]).rect.min.x, 100.0);
}

#[test]
fn hstack_align_center_centers_child_on_cross_axis() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .size((Sizing::FILL, Sizing::Fixed(100.0)))
        .show(&mut ui, |ui| {
            Frame::with_id("c")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .align(Align::CENTER)
                .show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let r = ui.tree.node(kids[0]).rect;
    // Cross axis is height (100); child is 20 tall → centered at (100-20)/2 = 40.
    assert_eq!(r.min.y, 40.0);
    assert_eq!(r.size.h, 20.0);
}

#[test]
fn negative_left_margin_spills_outside_slot() {
    // CSS-style negative margin: the widget reserves a smaller slot but renders
    // larger, shifted toward the negative side. Pin the math so future layout
    // tweaks don't regress it.
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut button_node = None;
    let root = HStack::new()
        .show(&mut ui, |ui| {
            button_node = Some(
                Button::with_id("spill")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(30.0)))
                    .margin((-10.0, 0.0, 0.0, 0.0))
                    .show(ui)
                    .node,
            );
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(kids.len(), 1);

    // Rendered rect (what the renderer paints, what hit-test uses) is shifted
    // 10px left of the slot and full Fixed-50 wide — i.e. spilled.
    let r = ui.tree.node(button_node.unwrap()).rect;
    assert_eq!(r.min.x, -10.0, "rendered rect spills 10px left of slot");
    assert_eq!(r.min.y, 0.0);
    assert_eq!(
        r.size.w, 50.0,
        "Fixed value is the rendered width, margin doesn't shrink it"
    );
    assert_eq!(r.size.h, 30.0);
}

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
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 200.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let left = ui.tree.node(kids[0]).rect;
    let right = ui.tree.node(kids[1]).rect;
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
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 200.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let short_btn = ui.tree.node(kids[0]).rect;
    let long_btn = ui.tree.node(kids[1]).rect;
    let body = ui.tree.node(kids[2]).rect;
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
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 100.0));
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.tree.node(kids[0]).rect.size.w, 100.0);
    assert_eq!(ui.tree.node(kids[1]).rect.size.w, 300.0);
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
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 100.0));
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.tree.node(kids[0]).rect.size.w, 200.0);
    assert_eq!(ui.tree.node(kids[1]).rect.size.w, 200.0);
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
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 100.0));
    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.tree.node(kids[0]).rect.size.w, 150.0);
    assert_eq!(ui.tree.node(kids[1]).rect.size.w, 250.0);
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
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 200.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let header = ui.tree.node(kids[0]).rect;
    let body = ui.tree.node(kids[1]).rect;
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
    // `layout::run` is forced to the surface size regardless of Sizing.
    let mut grid_node = None;
    let root = HStack::new()
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
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 200.0));
    let r = ui.tree.node(grid_node.unwrap()).rect;
    assert_eq!(r.size.w, 80.0, "hug grid collapses Fill col to 0");
    assert_eq!(r.size.h, 40.0);
}
