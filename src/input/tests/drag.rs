use crate::Ui;
use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn build_clickable(ui: &mut Ui) {
    Panel::hstack()
        .id(WidgetId::from_hash("target"))
        .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
        .sense(Sense::CLICK)
        .show(ui, |_| {});
}

fn build_draggable(ui: &mut Ui) {
    // Wider sense so press routing accepts non-left buttons. `clicks()`
    // is true for both CLICK and DRAG, so this still captures.
    Panel::hstack()
        .id(WidgetId::from_hash("target"))
        .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
        .sense(Sense::DRAG)
        .show(ui, |_| {});
}

fn id() -> WidgetId {
    WidgetId::from_hash("target")
}

fn delta(ui: &Ui, id: WidgetId, button: PointerButton) -> Option<Vec2> {
    ui.response_for(id).drag_delta_by(button)
}

fn started(ui: &Ui, id: WidgetId, button: PointerButton) -> bool {
    ui.response_for(id).drag_started_by(button)
}

fn dragged(ui: &Ui, id: WidgetId, button: PointerButton) -> bool {
    ui.response_for(id).dragged_by(button)
}

// ── Left-button drag ─────────────────────────────────────────────────

#[test]
fn drag_delta_none_before_press() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    assert_eq!(
        delta(&ui, id(), PointerButton::Left),
        None,
        "no press → no drag"
    );
}

#[test]
fn drag_delta_tracks_pointer_minus_press() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 30.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(80.0, 70.0)));

    assert_eq!(
        delta(&ui, id(), PointerButton::Left),
        Some(Vec2::new(60.0, 40.0)),
        "delta = current - press_pos",
    );
}

#[test]
fn drag_delta_persists_when_pointer_leaves_widget_rect() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 200.0)));

    assert_eq!(
        delta(&ui, id(), PointerButton::Left),
        Some(Vec2::new(250.0, 150.0)),
    );
}

#[test]
fn drag_delta_clears_on_release() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(30.0, 30.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(70.0, 70.0)));
    assert!(delta(&ui, id(), PointerButton::Left).is_some());

    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    assert_eq!(
        delta(&ui, id(), PointerButton::Left),
        None,
        "release ends the drag (active cleared)",
    );
}

#[test]
fn drag_delta_none_when_pointer_left_surface() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerLeft);

    assert_eq!(delta(&ui, id(), PointerButton::Left), None);
}

#[test]
fn drag_delta_only_for_active_widget() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_clickable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(60.0, 50.0)));

    let other = WidgetId::from_hash("other");
    assert_eq!(
        delta(&ui, other, PointerButton::Left),
        None,
        "only the captured widget sees the drag delta",
    );
}

// ── Non-left buttons ─────────────────────────────────────────────────

#[test]
fn middle_drag_tracks_pointer_minus_press_after_latch() {
    // Middle-button press anchors at (20, 30); pointer moves to
    // (80, 70). Travel = sqrt(60^2 + 40^2) = 72.1 px > DRAG_THRESHOLD
    // (4 px) so the drag latches.
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_draggable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 30.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Middle));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(80.0, 70.0)));

    assert_eq!(
        delta(&ui, id(), PointerButton::Middle),
        Some(Vec2::new(60.0, 40.0)),
    );
    assert!(
        started(&ui, id(), PointerButton::Middle),
        "drag-start edge must fire on the threshold-crossing move",
    );
    assert!(dragged(&ui, id(), PointerButton::Middle));
    // And the button-agnostic accessors point at the same drag.
    let r = ui.response_for(id());
    assert_eq!(r.drag_delta(), Some(Vec2::new(60.0, 40.0)));
    assert!(r.dragged());
    assert!(r.drag_started());
}

#[test]
fn middle_drag_does_not_expose_delta_below_threshold() {
    // Press + 3 px wiggle = no latch. `started` stays false and
    // `delta` is `None`, mirroring left-button semantics.
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_draggable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Middle));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(52.0, 51.0)));

    assert_eq!(delta(&ui, id(), PointerButton::Middle), None);
    assert!(!started(&ui, id(), PointerButton::Middle));
    assert!(!dragged(&ui, id(), PointerButton::Middle));
    assert!(!ui.response_for(id()).dragged());
}

#[test]
fn drag_started_is_one_frame_edge_then_clears_on_post_record() {
    // The `started` flag is a single-frame edge: true on the latching
    // move, false on subsequent moves even while the drag continues.
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_draggable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Middle));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(80.0, 50.0))); // latches
    assert!(started(&ui, id(), PointerButton::Middle));
    // A second frame (record-pass clears the edge); same gesture
    // continues.
    ui.run_at_acked(UVec2::new(200, 200), build_draggable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(100.0, 50.0)));
    assert!(
        !started(&ui, id(), PointerButton::Middle),
        "started must clear after one frame",
    );
    assert_eq!(
        delta(&ui, id(), PointerButton::Middle),
        Some(Vec2::new(50.0, 0.0)),
        "delta keeps tracking",
    );
}

#[test]
fn right_button_drag_also_latches() {
    // The drag-latch loop iterates every PointerButton, so right
    // drag works the same as left/middle.
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_draggable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Right));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(70.0, 40.0)));

    assert_eq!(
        delta(&ui, id(), PointerButton::Right),
        Some(Vec2::new(30.0, 0.0)),
    );
    assert!(started(&ui, id(), PointerButton::Right));
}

// ── Multi-button: priority-first wins; releasing it promotes the next ─

#[test]
fn left_wins_over_simultaneously_latched_middle() {
    // Both left and middle are latched on the same widget. Only one
    // drag is reported — the priority-first in `PointerButton::all()`
    // (left). `dragged_by(Middle)` is false even though the middle
    // press is still captured.
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(300, 300), build_draggable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 20.0))); // latches left
    ui.on_input(InputEvent::PointerPressed(PointerButton::Middle));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(100.0, 60.0))); // latches middle

    let r = ui.response_for(id());
    let d = r.drag.expect("a drag must be active");
    assert_eq!(
        d.button,
        PointerButton::Left,
        "left has priority over middle"
    );
    // Left was pressed at (20, 20); current pointer (100, 60).
    assert_eq!(d.delta, Vec2::new(80.0, 40.0));
    assert!(r.dragged_by(PointerButton::Left));
    assert!(
        !r.dragged_by(PointerButton::Middle),
        "middle is captured but not the active drag",
    );
}

#[test]
fn releasing_priority_button_promotes_lower_priority() {
    // After releasing left while middle is still held + latched, the
    // active drag transitions to middle without the user lifting
    // anything else.
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(300, 300), build_draggable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Middle));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(80.0, 60.0))); // both latch

    assert!(ui.response_for(id()).dragged_by(PointerButton::Left));

    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    let r = ui.response_for(id());
    assert!(
        r.dragged_by(PointerButton::Middle),
        "releasing left must promote middle to the active drag",
    );
    assert!(!r.dragged_by(PointerButton::Left));
    // Middle's anchor is the middle press position (same frame as
    // left's, so (20, 20)); delta = current (80, 60) - press (20, 20).
    assert_eq!(
        r.drag_delta_by(PointerButton::Middle),
        Some(Vec2::new(60.0, 40.0)),
    );
}

// ── Misses + zero-state ──────────────────────────────────────────────

#[test]
fn drag_zero_state_for_uncaptured_widget() {
    // A widget that didn't capture the press sees the zero state
    // regardless of which button is being dragged elsewhere.
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_draggable);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Middle));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(80.0, 70.0)));

    let other = WidgetId::from_hash("other");
    let r = ui.response_for(other);
    assert_eq!(r.drag_delta(), None);
    assert_eq!(r.drag_delta_by(PointerButton::Middle), None);
    assert!(!r.dragged());
    assert!(!r.dragged_by(PointerButton::Middle));
    assert!(!r.drag_started());
    assert!(!r.drag_started_by(PointerButton::Middle));
}

#[test]
fn drag_delta_none_when_press_missed_all_widgets() {
    // Outer non-clickable wraps a small clickable so the root doesn't
    // auto-fill the surface and swallow the press.
    let surface = UVec2::new(400, 400);
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("target"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .sense(Sense::CLICK)
                .show(ui, |_| {});
        });
    };
    let mut ui = Ui::for_test();
    ui.run_at_acked(surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(200.0, 200.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(250.0, 220.0)));
    ui.run_at_acked(surface, build);
    assert_eq!(delta(&ui, id(), PointerButton::Left), None);
}
