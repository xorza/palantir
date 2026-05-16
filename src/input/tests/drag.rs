use crate::Ui;
use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::test_support::new_ui;
use crate::ui::test_support::run_at_acked;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn build_clickable(ui: &mut Ui) {
    Panel::hstack()
        .id_salt("target")
        .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
        .sense(Sense::CLICK)
        .show(ui, |_| {});
}

fn id() -> WidgetId {
    WidgetId::from_hash("target")
}

#[test]
fn drag_delta_none_before_press() {
    let mut ui = new_ui();
    run_at_acked(&mut ui, UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    assert_eq!(ui.input.drag_delta(id()), None, "no press → no drag");
}

#[test]
fn drag_delta_tracks_pointer_minus_press() {
    let mut ui = new_ui();
    run_at_acked(&mut ui, UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 30.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(80.0, 70.0)));

    assert_eq!(
        ui.input.drag_delta(id()),
        Some(Vec2::new(60.0, 40.0)),
        "delta = current - press_pos"
    );
}

#[test]
fn drag_delta_persists_when_pointer_leaves_widget_rect() {
    let mut ui = new_ui();
    run_at_acked(&mut ui, UVec2::new(400, 400), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 200.0)));

    assert_eq!(ui.input.drag_delta(id()), Some(Vec2::new(250.0, 150.0)));
}

#[test]
fn drag_delta_clears_on_release() {
    let mut ui = new_ui();
    run_at_acked(&mut ui, UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(30.0, 30.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(70.0, 70.0)));
    assert!(ui.input.drag_delta(id()).is_some());

    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    assert_eq!(
        ui.input.drag_delta(id()),
        None,
        "release ends the drag (active cleared)"
    );
}

#[test]
fn drag_delta_none_when_pointer_left_surface() {
    let mut ui = new_ui();
    run_at_acked(&mut ui, UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerLeft);

    assert_eq!(ui.input.drag_delta(id()), None);
}

#[test]
fn drag_delta_only_for_active_widget() {
    let mut ui = new_ui();
    run_at_acked(&mut ui, UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(60.0, 50.0)));

    let other = WidgetId::from_hash("other");
    assert_eq!(
        ui.input.drag_delta(other),
        None,
        "only the captured widget sees the drag delta"
    );
}

#[test]
fn drag_delta_none_when_press_missed_all_widgets() {
    // Outer non-clickable wraps a small clickable so the root doesn't
    // auto-fill the surface and swallow the press.
    let surface = UVec2::new(400, 400);
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id_salt("target")
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .sense(Sense::CLICK)
                .show(ui, |_| {});
        });
    };
    let mut ui = new_ui();
    run_at_acked(&mut ui, surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(200.0, 200.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(250.0, 220.0)));
    run_at_acked(&mut ui, surface, build);
    assert_eq!(ui.input.drag_delta(id()), None);
}
