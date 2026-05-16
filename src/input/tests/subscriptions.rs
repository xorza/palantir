//! Off-target wake gates — `PointerSense` flags + specific key
//! chords. Pinned axes:
//!  * subscriber wakes the frame on events that otherwise wouldn't
//!    (press on inert surface, key with no focus);
//!  * no subscriber → no wake AND no entry in `frame_pointer_events`
//!    (the `any_mask` short-circuit gates the push);
//!  * pre-record clear drops stale subscriptions.

use crate::Ui;
use crate::input::InputEvent;
use crate::input::keyboard::{Key, Modifiers};
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::shortcut::Shortcut;
use crate::input::subscriptions::PointerSense;
use glam::{UVec2, Vec2};

fn empty(ui: &mut Ui) {
    use crate::forest::element::Configure;
    use crate::widgets::panel::Panel;
    Panel::vstack().id_salt("root").show(ui, |_| {});
}

fn empty_sub_buttons(ui: &mut Ui) {
    empty(ui);
    ui.subscribe_pointer(PointerSense::BUTTONS);
}

fn empty_sub_move(ui: &mut Ui) {
    empty(ui);
    ui.subscribe_pointer(PointerSense::MOVE);
}

fn empty_sub_escape(ui: &mut Ui) {
    empty(ui);
    ui.subscribe_key(Shortcut::key(Key::Escape));
}

#[test]
fn buttons_subscriber_wakes_press_on_inert() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), empty_sub_buttons);

    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let delta = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    assert!(delta.requests_repaint);

    let events = ui.pointer_events();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        PointerEvent::Down {
            pos,
            button: PointerButton::Left,
        } if pos == Vec2::new(50.0, 50.0)
    ));
}

#[test]
fn press_on_inert_with_no_subscriber_does_not_wake() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), empty);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let delta = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    assert!(!delta.requests_repaint);
    assert!(ui.pointer_events().is_empty());
}

#[test]
fn record_without_resubscribe_drops_wake() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), empty_sub_buttons);
    ui.run_at_acked(UVec2::new(200, 200), empty);

    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let delta = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    assert!(!delta.requests_repaint);
}

#[test]
fn press_and_release_both_captured() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), empty_sub_buttons);

    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let _ = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    let release = ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    assert!(release.requests_repaint);

    let events = ui.pointer_events();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], PointerEvent::Down { .. }));
    assert!(matches!(events[1], PointerEvent::Up { .. }));
}

/// `MOVE` wakes on every pointer move — even inert ones.
#[test]
fn move_subscriber_wakes_on_inert_move() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), empty_sub_move);

    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    assert!(delta.requests_repaint);

    let events = ui.pointer_events();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        PointerEvent::Move(p) if p == Vec2::new(50.0, 50.0)
    ));
}

/// `MOVE` not subscribed → no `Move` in the stream even
/// though hover may have changed. Hover-driven wake still fires
/// via the existing hit-test path; we're only checking the buffer.
#[test]
fn move_without_subscriber_does_not_log() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), empty);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    assert!(ui.pointer_events().is_empty());
}

#[test]
fn key_chord_subscriber_wakes_only_exact_chord() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), empty_sub_escape);
    assert!(ui.input.focused.is_none());

    let delta = ui.on_input(InputEvent::KeyDown {
        key: Key::Enter,
        repeat: false,
    });
    assert!(!delta.requests_repaint);

    // Alt+Escape: subscriber asked for bare Escape → no match.
    // (Avoid ctrl here: on macOS, raw Ctrl isn't represented in
    // `Shortcut`'s `Mods` vocabulary, so ctrl+Escape would *match*
    // Shortcut::key(Escape) — a documented platform compromise.)
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    let _ = ui.on_input(InputEvent::ModifiersChanged(alt));
    let delta = ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
    });
    assert!(!delta.requests_repaint);

    let _ = ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE));
    let delta = ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
    });
    assert!(delta.requests_repaint);
}

#[test]
fn pointer_events_drain_between_frames() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), empty_sub_buttons);

    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let _ = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    assert_eq!(ui.pointer_events().len(), 1);

    ui.run_at_acked(UVec2::new(200, 200), empty_sub_buttons);
    assert!(ui.pointer_events().is_empty());
}
