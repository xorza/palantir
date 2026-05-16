//! `InputDelta::requests_repaint` gating: pointer moves over inert
//! surfaces leave it false so the host can skip a frame entirely.

use crate::UiCore;
use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn build_hover_target(ui: &mut UiCore) {
    Panel::hstack()
        .id_salt("hot")
        .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
        .sense(Sense::HOVER)
        .show(ui, |_| {});
}

fn build_two_hover_targets(ui: &mut UiCore) {
    Panel::hstack()
        .id_salt("outer")
        .size((Sizing::Hug, Sizing::Hug))
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
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);
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
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);
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
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 300.0)));
    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    assert!(delta.requests_repaint);
}

#[test]
fn move_between_two_hover_targets_requests_repaint() {
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 200), build_two_hover_targets);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(150.0, 20.0)));
    assert!(delta.requests_repaint, "hovered widget changed → repaint");
}

#[test]
fn move_during_active_capture_requests_repaint() {
    let mut ui = UiCore::for_test();
    let build = |ui: &mut UiCore| {
        Panel::hstack()
            .id_salt("hot")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .sense(Sense::CLICK)
            .show(ui, |_| {});
    };
    ui.run_at_acked(UVec2::new(400, 400), build);
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
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let delta = ui.on_input(InputEvent::PointerLeft);
    assert!(delta.requests_repaint, "leave while hovered → repaint");
}

#[test]
fn pointer_left_with_nothing_active_does_not_request_repaint() {
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);
    // Never moved over the target, never captured → leaving is a no-op.
    let delta = ui.on_input(InputEvent::PointerLeft);
    assert!(!delta.requests_repaint);
}

/// `Text` wakes only when a focused widget would consume it OR a
/// `KeyboardSense::TEXT` subscriber asked for it. `ModifiersChanged`
/// wakes only with a `KeyboardSense::MODIFIER` subscriber.
#[test]
fn non_pointer_events_wake_on_focus_or_subscription() {
    use crate::input::keyboard::{Modifiers, TextChunk};
    use crate::input::subscriptions::KeyboardSense;
    use crate::primitives::widget_id::WidgetId;
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);

    // No focus, no subscription → no wake.
    assert!(
        !ui.on_input(InputEvent::Text(TextChunk::new("a").unwrap()))
            .requests_repaint,
    );
    assert!(
        !ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE))
            .requests_repaint,
    );

    // Focus held → Text wakes.
    ui.input.focused = Some(WidgetId::from_hash("editor"));
    assert!(
        ui.on_input(InputEvent::Text(TextChunk::new("b").unwrap()))
            .requests_repaint,
    );
    ui.input.focused = None;

    // KeyboardSense subscribers → Text + ModifiersChanged wake.
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        build_hover_target(ui);
        ui.subscribe_keyboard(KeyboardSense::TEXT | KeyboardSense::MODIFIER);
    });
    assert!(
        ui.on_input(InputEvent::Text(TextChunk::new("c").unwrap()))
            .requests_repaint,
    );
    assert!(
        ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE))
            .requests_repaint,
    );
}

/// `KeyDown` wakes only when a focused widget would consume it or
/// a global chord subscriber asked for it. Idle keys (no focus,
/// no subscriber) skip the frame under `OnDelta`.
#[test]
fn keydown_wakes_only_when_focus_or_subscription_exists() {
    use crate::input::keyboard::Key;
    use crate::input::shortcut::Shortcut;
    use crate::input::subscriptions::PointerSense;
    use crate::primitives::widget_id::WidgetId;
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);

    // No focus, no chord sub → no wake.
    let delta = ui.on_input(InputEvent::KeyDown {
        key: Key::Enter,
        repeat: false,
    });
    assert!(!delta.requests_repaint, "idle key must skip the frame");

    // With focus held → wake.
    ui.input.focused = Some(WidgetId::from_hash("editor"));
    let delta = ui.on_input(InputEvent::KeyDown {
        key: Key::Enter,
        repeat: false,
    });
    assert!(delta.requests_repaint);

    // No focus, but chord subscriber → wake. Subscriptions are
    // cleared pre-record, so re-record with the sub re-asserted.
    ui.input.focused = None;
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        build_hover_target(ui);
        ui.subscribe_key(Shortcut::key(Key::Escape));
        // Also reassert this so it survives — but we only test Escape below.
        let _ = PointerSense::BUTTONS;
    });
    let delta = ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
    });
    assert!(delta.requests_repaint);
}

/// Press + release on an inert surface with no focus and no popup is
/// a true no-op — no hover hit, no click target, no focus change,
/// no capture to settle. Under `InputPolicy::OnDelta` the host can
/// skip the frame entirely.
#[test]
fn press_release_on_inert_with_no_focus_does_not_request_repaint() {
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);
    // Pointer at (200, 200): well outside the 100×100 hover target.
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(200.0, 200.0)));
    assert!(
        !ui.on_input(InputEvent::PointerPressed(PointerButton::Left))
            .requests_repaint,
        "press on inert surface, no focus → no repaint",
    );
    assert!(
        !ui.on_input(InputEvent::PointerReleased(PointerButton::Left))
            .requests_repaint,
        "stray release (no capture) → no repaint",
    );
}

/// Click outside any focusable widget while focus is held by a
/// `Focusable` widget clears focus under the default
/// `FocusPolicy::ClearOnMiss` — observably a visual change, so the
/// press must request repaint even though it didn't hit anything
/// clickable.
#[test]
fn press_on_inert_clears_focus_and_requests_repaint() {
    use crate::primitives::widget_id::WidgetId;
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(400, 400), build_hover_target);
    // Forge a focused widget — emulating a prior TextEdit interaction.
    ui.input.focused = Some(WidgetId::from_hash("editor"));
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(200.0, 200.0)));
    let delta = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    assert!(
        delta.requests_repaint,
        "press on inert with prior focus → focus clear → repaint",
    );
    assert!(ui.input.focused.is_none(), "focus must be cleared");
}
