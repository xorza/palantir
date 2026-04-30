use crate::primitives::{Rect, Sizing};
use crate::widgets::{Button, HStack, VStack};
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

    let a = ui.tree.node(kids[0]).rect;
    assert_eq!(a.min.x, 0.0);
    assert_eq!(a.min.y, 0.0);
    assert_eq!(a.size.w, 32.0);
    assert_eq!(a.size.h, 32.0);

    let b = ui.tree.node(kids[1]).rect;
    assert_eq!(b.min.x, 32.0);
    assert_eq!(b.size.w, 100.0);
    assert_eq!(b.size.h, 32.0);
}

#[test]
fn vstack_with_fill_distributes_remainder() {
    let mut ui = Ui::new();
    ui.begin_frame();

    let root = VStack::new()
        .show(&mut ui, |ui| {
            Button::new().size((Sizing::Hug, 50.0)).show(ui);
            Button::new().size((Sizing::Hug, Sizing::Fill)).show(ui);
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
