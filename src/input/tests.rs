use crate::element::Element;
use crate::input::{InputEvent, PointerButton};
use crate::primitives::{Rect, Sense, Sizing};
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
fn stack_with_sense_none_passes_clicks_through() {
    // HStack default Sense::NONE — clicking on its background (between children)
    // doesn't fire `clicked` on the stack. Clicking on a child still fires on the child.
    let mut ui = Ui::new();
    ui.begin_frame();
    let stack_node = HStack::new()
        .padding(20.0) // creates "background" area to click
        .show(&mut ui, |ui| {
            Button::with_id("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    layout::run(&mut ui.tree, stack_node, Rect::new(0.0, 0.0, 200.0, 100.0));
    ui.end_frame();

    // Press inside the HStack's padding (not over any child).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(5.0, 5.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame();
    let mut child_clicked = false;
    let stack_resp = HStack::new().padding(20.0).show(&mut ui, |ui| {
        child_clicked = Button::with_id("inside")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .show(ui)
            .clicked();
    });
    assert!(
        !stack_resp.clicked(),
        "non-sensing stack does not capture clicks"
    );
    assert!(
        !child_clicked,
        "click on stack background doesn't reach child"
    );
}

#[test]
fn stack_with_sense_click_captures_clicks() {
    // Opt-in: HStack::sense(Sense::CLICK) makes the container clickable.
    // Use `with_id` so the stack has the same WidgetId on both frames; otherwise
    // `auto_stable` would give different ids (different call sites in the test).
    let mut ui = Ui::new();
    ui.begin_frame();
    let stack_node = HStack::with_id("clickable_card")
        .padding(20.0)
        .sense(Sense::CLICK)
        .show(&mut ui, |ui| {
            Button::with_id("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    layout::run(&mut ui.tree, stack_node, Rect::new(0.0, 0.0, 200.0, 100.0));
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(5.0, 5.0))); // padding area
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame();
    let stack_resp = HStack::with_id("clickable_card")
        .padding(20.0)
        .sense(Sense::CLICK)
        .show(&mut ui, |ui| {
            Button::with_id("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    assert!(
        stack_resp.clicked(),
        "stack with Sense::CLICK fires on background click"
    );
}

#[test]
fn stack_with_sense_hover_reports_hover_but_passes_clicks_through() {
    // Sense::HOVER: visible to hover state but transparent to click capture.
    // Useful for tooltips, cursor changes, row highlights.
    let mut ui = Ui::new();
    ui.begin_frame();
    let stack_node = HStack::with_id("hover_only")
        .padding(20.0)
        .sense(Sense::HOVER)
        .show(&mut ui, |ui| {
            Button::with_id("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    layout::run(&mut ui.tree, stack_node, Rect::new(0.0, 0.0, 200.0, 100.0));
    ui.end_frame();

    // Move pointer over stack's padding area (not over the button).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(5.0, 5.0)));

    // Press + release on the same spot.
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame();
    let mut child_clicked = false;
    let stack_resp = HStack::with_id("hover_only")
        .padding(20.0)
        .sense(Sense::HOVER)
        .show(&mut ui, |ui| {
            child_clicked = Button::with_id("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui)
                .clicked();
        });

    assert!(
        stack_resp.hovered(),
        "Sense::HOVER stack reports hovered=true"
    );
    assert!(
        !stack_resp.clicked(),
        "Sense::HOVER does not capture clicks"
    );
    assert!(
        !child_clicked,
        "no clickable widget under cursor → no click anywhere"
    );
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
