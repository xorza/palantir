use crate::input::{InputEvent, PointerButton};
use crate::primitives::{Rect, Sizing};
use crate::widgets::{Button, HStack};
use crate::{Ui, layout};
use glam::Vec2;

#[test]
fn input_state_press_release_emits_click() {
    // Drive the input state machine without any windowing toolkit. Two frames:
    // frame 1 lays out a button so its rect lands in `last_rects`; then a
    // press+release pair over its rect produces `clicked = true` on frame 2.
    let mut ui = Ui::new();

    // Frame 1: build, layout, end_frame to populate last_rects.
    ui.begin_frame();
    let root = HStack::new()
        .show(&mut ui, |ui| {
            Button::with_id("target")
                .label("hi")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    let surface = Rect::new(0.0, 0.0, 200.0, 80.0);
    layout::run(&mut ui.tree, root, surface);
    ui.end_frame();

    // Press inside the button, release inside.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    // Frame 2: rebuild; widgets should observe the click in build_ui.
    ui.begin_frame();
    let mut got_click = false;
    HStack::new().show(&mut ui, |ui| {
        let r = Button::with_id("target")
            .label("hi")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
        got_click = r.clicked();
    });
    assert!(got_click, "press+release inside button rect should click");

    // Click does not stick: next frame without input must clear it.
    let root2 = ui.root();
    layout::run(&mut ui.tree, root2, surface);
    ui.end_frame();
    ui.begin_frame();
    let mut still_clicking = false;
    HStack::new().show(&mut ui, |ui| {
        still_clicking = Button::with_id("target")
            .label("hi")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui)
            .clicked();
    });
    assert!(!still_clicking, "click is one-shot");
}

#[test]
fn input_state_release_outside_does_not_click() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = HStack::new()
        .show(&mut ui, |ui| {
            Button::with_id("target")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 80.0));
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0))); // inside
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 20.0))); // outside
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame();
    let mut got_click = false;
    HStack::new().show(&mut ui, |ui| {
        got_click = Button::with_id("target")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui)
            .clicked();
    });
    assert!(
        !got_click,
        "release outside the original widget cancels click"
    );
}
