use crate::primitives::{Color, Rect, Sense, Sizing};
use crate::shape::Shape;
use crate::widgets::{Frame, HStack};
use crate::{Ui, layout};

#[test]
fn frame_paints_a_single_rounded_rect() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut frame_node = None;
    HStack::new().show(&mut ui, |ui| {
        frame_node = Some(
            Frame::with_id("decoration")
                .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .radius(6.0)
                .show(ui)
                .node,
        );
    });
    let root = ui.root();
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));

    let shapes = ui.tree.shapes_of(frame_node.unwrap());
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], Shape::RoundedRect { .. }));

    // Default sense is None — frame is not a hit-test target.
    let r = ui.tree.node(frame_node.unwrap()).rect;
    assert_eq!(r.size.w, 80.0);
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn frame_with_sense_click_is_clickable() {
    use crate::input::{InputEvent, PointerButton};
    use glam::Vec2;

    let mut ui = Ui::new();
    ui.begin_frame();
    HStack::new().show(&mut ui, |ui| {
        Frame::with_id("hitbox")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .sense(Sense::CLICK)
            .show(ui);
    });
    let root = ui.root();
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 100.0));
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 25.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame();
    let mut clicked = false;
    HStack::new().show(&mut ui, |ui| {
        clicked = Frame::with_id("hitbox")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .sense(Sense::CLICK)
            .show(ui)
            .clicked();
    });
    assert!(clicked);
}
