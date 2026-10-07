//! `InputDelta::repaint_requested` gating: pointer moves over inert surfaces leave it false.

use crate::Ui;
use crate::input::input_event::InputEvent;
use crate::input::input_state::tests::forged_focus;
use crate::input::sense::Sense;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
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
    h.move_to(Vec2::new(200.0, 200.0));
    let delta = h.move_to(Vec2::new(250.0, 220.0));
    assert!(
        !delta.repaint_requested,
        "move over empty surface: no repaint"
    );
}

#[test]
fn move_within_same_hovered_widget_does_not_request_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    let enter = h.move_to(Vec2::new(20.0, 20.0));
    assert!(enter.repaint_requested, "enter hover target → repaint");
    let inside = h.move_to(Vec2::new(50.0, 50.0));
    assert!(
        !inside.repaint_requested,
        "move inside same hover target: no repaint",
    );
}

#[test]
fn move_from_inert_into_hover_target_requests_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    h.move_to(Vec2::new(300.0, 300.0));
    let delta = h.move_to(Vec2::new(20.0, 20.0));
    assert!(delta.repaint_requested);
}

#[test]
fn move_between_two_hover_targets_requests_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.frame(build_two_hover_targets);
    h.move_to(Vec2::new(20.0, 20.0));
    let delta = h.move_to(Vec2::new(150.0, 20.0));
    assert!(delta.repaint_requested, "hovered widget changed → repaint");
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
    // Tiny move under the drag threshold, but `active.is_some()` so the widget reads drag_delta.
    let delta = h.move_to(Vec2::new(51.0, 51.0));
    assert!(
        delta.repaint_requested,
        "move while capture is active → repaint (drag widgets consume delta)",
    );
}

#[test]
fn pointer_left_after_hover_requests_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    h.move_to(Vec2::new(50.0, 50.0));
    let delta = h.pointer_left();
    assert!(delta.repaint_requested, "leave while hovered → repaint");
}

#[test]
fn pointer_left_with_nothing_active_does_not_request_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    let delta = h.pointer_left();
    assert!(!delta.repaint_requested);
}

/// `ModifiersChanged` wakes only with a `KeyboardWake::MODIFIER` watcher; focus alone does not.
#[test]
fn modifiers_wake_only_for_a_watcher() {
    use crate::input::keyboard::modifiers::Modifiers;
    use crate::input::watch::KeyboardWake;
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);

    assert!(
        !h.on_input(InputEvent::ModifiersChanged(Modifiers::NONE))
            .repaint_requested,
    );

    h.set_focus(forged_focus());
    assert!(
        !h.on_input(InputEvent::ModifiersChanged(Modifiers::SHIFT))
            .repaint_requested,
    );
    h.clear_focus();

    h.frame(|ui| {
        build_hover_target(ui);
        ui.watch_keyboard(KeyboardWake::MODIFIER);
    });
    assert!(
        h.on_input(InputEvent::ModifiersChanged(Modifiers::NONE))
            .repaint_requested,
    );
}

/// `KeyDown` wakes only when a focused widget would consume it or a chord watcher asked for it.
#[test]
fn keydown_wakes_only_when_focus_or_watch_exists() {
    use crate::input::keyboard::key::Key;
    use crate::input::shortcut::Shortcut;
    use crate::input::watch::PointerWake;
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);

    let delta = h.key(Key::Enter);
    assert!(!delta.repaint_requested, "idle key must skip the frame");
    assert!(
        !h.ui.input_mut().take_action_flag(),
        "unrouted key must not schedule a settling pass",
    );

    h.set_focus(forged_focus());
    let delta = h.key(Key::Enter);
    assert!(delta.repaint_requested);

    // Watches are cleared pre-record, so re-assert it.
    h.clear_focus();
    h.frame(|ui| {
        build_hover_target(ui);
        ui.watch_key(Shortcut::key(Key::Escape));
        // Keeps the `PointerWake` import used.
        let _ = PointerWake::BUTTONS;
    });
    let delta = h.key(Key::Escape);
    assert!(delta.repaint_requested);
}

/// Press and release on an inert surface with no focus or popup is a true no-op: the host can skip the frame.
#[test]
fn press_release_on_inert_with_no_focus_does_not_request_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    h.move_to(Vec2::new(200.0, 200.0));
    assert!(
        !h.press().repaint_requested,
        "press on inert surface, no focus → no repaint",
    );
    assert!(
        !h.release().repaint_requested,
        "stray release (no capture) → no repaint",
    );
    assert!(
        !h.ui.input_mut().take_action_flag(),
        "unrouted button events must not schedule a settling pass",
    );
}

/// A click outside any focusable widget clears focus under `FocusPolicy::ClearOnMiss`: a visual change.
#[test]
fn press_on_inert_clears_focus_and_requests_repaint() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(build_hover_target);
    h.set_focus(forged_focus());
    h.move_to(Vec2::new(200.0, 200.0));
    let delta = h.press();
    assert!(
        delta.repaint_requested,
        "press on inert with prior focus → focus clear → repaint",
    );
    assert_eq!(h.focus(), None, "focus must be cleared");
}

/// A bare modifier press (`Key::Other`, no text) is nothing a focused widget acts on: it neither wakes nor
/// settles a frame; a typed key does both.
#[test]
fn a_bare_modifier_press_wakes_nothing_while_a_widget_is_focused() {
    use crate::input::keyboard::key::Key;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(build_hover_target);
    h.set_focus(WidgetId::from_hash("hot"));
    h.frame(build_hover_target);

    let modifier = h.key(Key::Other);
    assert!(!modifier.repaint_requested, "a bare modifier wakes nothing");
    assert!(!h.ui.input_mut().take_action_flag(), "and settles nothing");

    let typed = h.key(Key::Char('c'));
    assert!(
        typed.repaint_requested,
        "control: a typed key reaches the focus"
    );
    assert!(h.ui.input_mut().take_action_flag(), "and settles");
}
