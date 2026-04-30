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
                .size((Sizing::Fill { weight: 1.0 }, Sizing::Hug))
                .show(ui);
            Frame::with_id("b")
                .size((Sizing::Fill { weight: 3.0 }, Sizing::Hug))
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
                .align(Align::Center)
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
