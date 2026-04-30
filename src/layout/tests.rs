use crate::element::Element;
use crate::primitives::{Align, Rect, Sizing};
use crate::widgets::{Button, Frame, HStack, VStack};
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
