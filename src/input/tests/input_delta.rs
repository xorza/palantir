//! `InputDelta::requests_repaint` gating: pointer moves over inert
//! surfaces leave it false so the host can skip a frame entirely.

use crate::Ui;
use crate::forest::element::Configure;
use crate::input::sense::Sense;
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::sizing::Sizing;
use crate::support::testing::run_at_acked;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn build_hover_target(ui: &mut Ui) {
    // Outer Fill panel with no sense gives us an inert background;
    // the inner Fixed(100, 100) hover target sits at top-left.
    Panel::hstack()
        .id_salt("outer")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::hstack()
                .id_salt("hot")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .sense(Sense::HOVER)
                .show(ui, |_| {});
        });
}

fn build_two_hover_targets(ui: &mut Ui) {
    Panel::hstack()
        .id_salt("outer")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::hstack()
                .id_salt("a")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .sense(Sense::HOVER)
                .show(ui, |_| {});
            Panel::hstack()
                .id_salt("b")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .sense(Sense::HOVER)
                .show(ui, |_| {});
        });
}

#[test]
fn move_over_inert_surface_does_not_request_repaint() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(400, 400), build_hover_target);
    // Both positions are outside the hover target → hovered stays None.
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(200.0, 200.0)));
    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(250.0, 220.0)));
    assert!(
        !delta.requests_repaint,
        "move over empty surface: no repaint"
    );
}

#[test]
fn move_within_same_hovered_widget_does_not_request_repaint() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(400, 400), build_hover_target);
    // First move: empty → over target. Repaint expected.
    let enter = ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    assert!(enter.requests_repaint, "enter hover target → repaint");
    // Second move: still over target. No hover change.
    let inside = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    assert!(
        !inside.requests_repaint,
        "move inside same hover target: no repaint",
    );
}

#[test]
fn move_from_inert_into_hover_target_requests_repaint() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(400, 400), build_hover_target);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 300.0)));
    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    assert!(delta.requests_repaint);
}

#[test]
fn move_between_two_hover_targets_requests_repaint() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(400, 200), build_two_hover_targets);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(150.0, 20.0)));
    assert!(delta.requests_repaint, "hovered widget changed → repaint");
}

#[test]
fn move_during_active_capture_requests_repaint() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id_salt("outer")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::hstack()
                    .id_salt("hot")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                    .sense(Sense::CLICK)
                    .show(ui, |_| {});
            });
    };
    run_at_acked(&mut ui, UVec2::new(400, 400), build);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let _ = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    // Tiny move (under drag threshold), still inside the same widget.
    // No hover change — but `active.is_some()` so widget reads drag_delta.
    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(51.0, 51.0)));
    assert!(
        delta.requests_repaint,
        "move while capture is active → repaint (drag widgets consume delta)",
    );
}

#[test]
fn pointer_left_after_hover_requests_repaint() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(400, 400), build_hover_target);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let delta = ui.on_input(InputEvent::PointerLeft);
    assert!(delta.requests_repaint, "leave while hovered → repaint");
}

#[test]
fn pointer_left_with_nothing_active_does_not_request_repaint() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(400, 400), build_hover_target);
    // Never moved over the target, never captured → leaving is a no-op.
    let delta = ui.on_input(InputEvent::PointerLeft);
    assert!(!delta.requests_repaint);
}

#[test]
fn non_pointer_events_request_repaint() {
    use crate::input::keyboard::{Key, Modifiers, TextChunk};
    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(400, 400), build_hover_target);
    assert!(
        ui.on_input(InputEvent::KeyDown {
            key: Key::Enter,
            repeat: false,
        })
        .requests_repaint,
    );
    assert!(
        ui.on_input(InputEvent::Text(TextChunk::new("a").unwrap()))
            .requests_repaint,
    );
    assert!(
        ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE))
            .requests_repaint,
    );
    // Click on empty surface still repaints (focus policy may clear focus).
    assert!(
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left))
            .requests_repaint,
    );
    assert!(
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left))
            .requests_repaint,
    );
}
