//! `InputDelta::requests_repaint` gating: pointer moves over inert
//! surfaces leave it false so the host can skip a frame entirely.

use crate::Ui;
use crate::input::InputEvent;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn build_hover_target(ui: &mut Ui) {
    Panel::hstack()
        .id(WidgetId::from_hash("hot"))
        .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
        .sense(Sense::HOVER)
        .show(ui, |_| {});
}

fn build_two_hover_targets(ui: &mut Ui) {
    Panel::hstack()
        .id(WidgetId::from_hash("outer"))
        .size((Sizing::HUG, Sizing::HUG))
        .show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("a"))
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .sense(Sense::HOVER)
                .show(ui, |_| {});
            Panel::hstack()
                .id(WidgetId::from_hash("b"))
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .sense(Sense::HOVER)
                .show(ui, |_| {});
        });
}

#[test]
fn move_over_inert_surface_does_not_request_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    // Both positions are outside the hover target → hovered stays None.
    h.move_to(Vec2::new(200.0, 200.0));
    let delta = h.move_to(Vec2::new(250.0, 220.0));
    assert!(
        !delta.requests_repaint,
        "move over empty surface: no repaint"
    );
}

#[test]
fn move_within_same_hovered_widget_does_not_request_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    // First move: empty → over target. Repaint expected.
    let enter = h.move_to(Vec2::new(20.0, 20.0));
    assert!(enter.requests_repaint, "enter hover target → repaint");
    // Second move: still over target. No hover change.
    let inside = h.move_to(Vec2::new(50.0, 50.0));
    assert!(
        !inside.requests_repaint,
        "move inside same hover target: no repaint",
    );
}

#[test]
fn move_from_inert_into_hover_target_requests_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    h.move_to(Vec2::new(300.0, 300.0));
    let delta = h.move_to(Vec2::new(20.0, 20.0));
    assert!(delta.requests_repaint);
}

#[test]
fn move_between_two_hover_targets_requests_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.frame(build_two_hover_targets);
    h.move_to(Vec2::new(20.0, 20.0));
    let delta = h.move_to(Vec2::new(150.0, 20.0));
    assert!(delta.requests_repaint, "hovered widget changed → repaint");
}

#[test]
fn move_during_active_capture_requests_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("hot"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .sense(Sense::CLICK)
            .show(ui, |_| {});
    };
    h.frame(build);
    h.press_at(Vec2::new(50.0, 50.0));
    // Tiny move (under drag threshold), still inside the same widget.
    // No hover change — but `active.is_some()` so widget reads drag_delta.
    let delta = h.move_to(Vec2::new(51.0, 51.0));
    assert!(
        delta.requests_repaint,
        "move while capture is active → repaint (drag widgets consume delta)",
    );
}

#[test]
fn pointer_left_after_hover_requests_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    h.move_to(Vec2::new(50.0, 50.0));
    let delta = h.on_input(InputEvent::PointerLeft);
    assert!(delta.requests_repaint, "leave while hovered → repaint");
}

#[test]
fn pointer_left_with_nothing_active_does_not_request_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    // Never moved over the target, never captured → leaving is a no-op.
    let delta = h.on_input(InputEvent::PointerLeft);
    assert!(!delta.requests_repaint);
}

/// `Text` wakes only when a focused widget would consume it OR a
/// `KeyboardWake::TEXT` watcher asked for it. `ModifiersChanged`
/// wakes only with a `KeyboardWake::MODIFIER` watcher.
#[test]
fn non_pointer_events_wake_on_focus_or_watch() {
    use crate::input::keyboard::{Modifiers, TextChunk};
    use crate::input::watch::KeyboardWake;
    use crate::primitives::widget_id::WidgetId;
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);

    // No focus, no watch → no wake.
    assert!(
        !h.on_input(InputEvent::Text(TextChunk::new("a").unwrap()))
            .requests_repaint,
    );
    assert!(
        !h.ui.input.take_action_flag(),
        "unrouted text must not schedule a settling pass",
    );
    assert!(
        !h.on_input(InputEvent::ModifiersChanged(Modifiers::NONE))
            .requests_repaint,
    );

    // Focus held → Text wakes.
    h.ui.input.focused = Some(WidgetId::from_hash("editor"));
    assert!(
        h.on_input(InputEvent::Text(TextChunk::new("b").unwrap()))
            .requests_repaint,
    );
    h.ui.input.focused = None;

    // KeyboardWake watchers → Text + ModifiersChanged wake.
    h.frame(|ui| {
        build_hover_target(ui);
        ui.watch_keyboard(KeyboardWake::TEXT | KeyboardWake::MODIFIER);
    });
    assert!(
        h.on_input(InputEvent::Text(TextChunk::new("c").unwrap()))
            .requests_repaint,
    );
    assert!(
        h.on_input(InputEvent::ModifiersChanged(Modifiers::NONE))
            .requests_repaint,
    );
}

/// `KeyDown` wakes only when a focused widget would consume it or
/// a global chord watcher asked for it. Idle keys (no focus,
/// no watcher) skip the frame under `OnDelta`.
#[test]
fn keydown_wakes_only_when_focus_or_watch_exists() {
    use crate::input::keyboard::Key;
    use crate::input::shortcut::Shortcut;
    use crate::input::watch::PointerWake;
    use crate::primitives::widget_id::WidgetId;
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);

    // No focus, no chord sub → no wake.
    let delta = h.key(Key::Enter);
    assert!(!delta.requests_repaint, "idle key must skip the frame");
    assert!(
        !h.ui.input.take_action_flag(),
        "unrouted key must not schedule a settling pass",
    );

    // With focus held → wake.
    h.ui.input.focused = Some(WidgetId::from_hash("editor"));
    let delta = h.key(Key::Enter);
    assert!(delta.requests_repaint);

    // No focus, but chord watcher → wake. Watches are
    // cleared pre-record, so re-record with the sub re-asserted.
    h.ui.input.focused = None;
    h.frame(|ui| {
        build_hover_target(ui);
        ui.watch_key(Shortcut::key(Key::Escape));
        // Also reassert this so it survives — but we only test Escape below.
        let _ = PointerWake::BUTTONS;
    });
    let delta = h.key(Key::Escape);
    assert!(delta.requests_repaint);
}

/// Press + release on an inert surface with no focus and no popup is
/// a true no-op — no hover hit, no click target, no focus change,
/// no capture to settle. Under `InputPolicy::OnDelta` the host can
/// skip the frame entirely.
#[test]
fn press_release_on_inert_with_no_focus_does_not_request_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    // Pointer at (200, 200): well outside the 100×100 hover target.
    h.move_to(Vec2::new(200.0, 200.0));
    assert!(
        !h.press().requests_repaint,
        "press on inert surface, no focus → no repaint",
    );
    assert!(
        !h.release().requests_repaint,
        "stray release (no capture) → no repaint",
    );
    assert!(
        !h.ui.input.take_action_flag(),
        "unrouted button events must not schedule a settling pass",
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
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    // Forge a focused widget — emulating a prior TextEdit interaction.
    h.ui.input.focused = Some(WidgetId::from_hash("editor"));
    h.move_to(Vec2::new(200.0, 200.0));
    let delta = h.press();
    assert!(
        delta.requests_repaint,
        "press on inert with prior focus → focus clear → repaint",
    );
    assert!(h.ui.input.focused.is_none(), "focus must be cleared");
}
