use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::support::testing::ui_at;
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
    let mut ui = ui_at(UVec2::new(200, 200));
    build_clickable(&mut ui);
    ui.record_phase();
    ui.paint_phase();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    assert_eq!(ui.input.drag_delta(id()), None, "no press → no drag");
}

#[test]
fn drag_delta_tracks_pointer_minus_press() {
    let mut ui = ui_at(UVec2::new(200, 200));
    build_clickable(&mut ui);
    ui.record_phase();
    ui.paint_phase();
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
    // The whole point of "rect-independent": once captured, the
    // pointer can wander outside the widget and the delta keeps
    // tracking. Pin so a future tightening doesn't gate drag on
    // staying inside the originating rect.
    let mut ui = ui_at(UVec2::new(400, 400));
    build_clickable(&mut ui);
    ui.record_phase();
    ui.paint_phase();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    // Move well outside the 100x100 widget rect.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 200.0)));

    assert_eq!(ui.input.drag_delta(id()), Some(Vec2::new(250.0, 150.0)));
}

#[test]
fn drag_delta_clears_on_release() {
    let mut ui = ui_at(UVec2::new(200, 200));
    build_clickable(&mut ui);
    ui.record_phase();
    ui.paint_phase();
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
    let mut ui = ui_at(UVec2::new(200, 200));
    build_clickable(&mut ui);
    ui.record_phase();
    ui.paint_phase();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerLeft);

    // press_pos kept; pointer.pos cleared → no current delta
    // available. (A polish pass could freeze last_pos so drags can
    // continue past the surface edge — defer.)
    assert_eq!(ui.input.drag_delta(id()), None);
}

#[test]
fn drag_delta_only_for_active_widget() {
    let mut ui = ui_at(UVec2::new(200, 200));
    build_clickable(&mut ui);
    ui.record_phase();
    ui.paint_phase();
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
    // Press over empty space ⇒ no widget captures, press_pos stays
    // None. A subsequent move doesn't synthesize a drag. Wrap the
    // small clickable in an outer non-clickable panel so the root
    // doesn't auto-fill the surface and swallow the press.
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
    let mut ui = ui_at(surface);
    build(&mut ui);
    ui.record_phase();
    ui.paint_phase();
    ui.begin_frame(Display::from_physical(surface, 1.0));
    build(&mut ui);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(200.0, 200.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(250.0, 220.0)));
    assert_eq!(ui.input.drag_delta(id()), None);
    ui.record_phase();
    ui.paint_phase();
}
