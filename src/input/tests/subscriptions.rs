//! Off-target wake gates — `PointerSense` flags + specific key
//! chords. Pinned axes:
//!  * subscriber wakes the frame on events that otherwise wouldn't
//!    (press on inert surface, key with no focus);
//!  * no subscriber → no wake AND no entry in `frame_pointer_events`
//!    (the `any_mask` short-circuit gates the push);
//!  * pre-record clear drops stale subscriptions.
use crate::primitives::widget_id::WidgetId;

use crate::Ui;
use crate::input::InputEvent;
use crate::input::keyboard::{Key, Modifiers};
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::policy::InputPolicy;
use crate::input::shortcut::Shortcut;
use crate::input::subscriptions::PointerSense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::scene::node::Configure;
use crate::shape::Shape;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn empty(ui: &mut Ui) {
    Panel::vstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |_| {});
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
    ui.run_at(UVec2::new(200, 200), empty_sub_buttons);

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
    ui.run_at(UVec2::new(200, 200), empty);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let delta = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    assert!(!delta.requests_repaint);
    assert!(ui.pointer_events().is_empty());
}

#[test]
fn record_without_resubscribe_drops_wake() {
    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(200, 200), empty_sub_buttons);
    ui.run_at(UVec2::new(200, 200), empty);

    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let delta = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    assert!(!delta.requests_repaint);
}

#[test]
fn press_and_release_both_captured() {
    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(200, 200), empty_sub_buttons);

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
    ui.run_at(UVec2::new(200, 200), empty_sub_move);

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
    ui.run_at(UVec2::new(200, 200), empty);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    assert!(ui.pointer_events().is_empty());
}

#[test]
fn scroll_subscriber_receives_an_event_without_creating_a_widget_delta() {
    fn empty_sub_scroll(ui: &mut Ui) {
        empty(ui);
        ui.subscribe_pointer(PointerSense::SCROLL);
    }

    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(200, 200), empty_sub_scroll);
    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let delta = ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 7.0)));

    assert!(delta.requests_repaint);
    assert!(ui.input.frame_target_deltas.is_empty());
    assert!(matches!(
        ui.pointer_events(),
        [PointerEvent::Scroll {
            pos,
            pixels,
            lines,
        }] if *pos == Vec2::new(50.0, 50.0)
            && *pixels == Vec2::new(0.0, 7.0)
            && *lines == Vec2::ZERO
    ));
}

/// Reading `Ui::pointer_pos` during record auto-asserts `MOVE`: record
/// output derived from the raw pointer may change on any move, so moves
/// must wake even over an inert surface. A pass that stops reading
/// drops the wake like any other lapsed subscription — the staleness
/// this pins: a pointer-proximity highlight painted from `pointer_pos`
/// must not freeze on screen when the hover target stops changing.
#[test]
fn pointer_pos_read_asserts_move_subscription() {
    fn empty_reads_pointer(ui: &mut Ui) {
        empty(ui);
        let _ = ui.pointer_pos();
    }

    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(200, 200), empty_reads_pointer);
    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    assert!(
        delta.requests_repaint,
        "a record pass that read pointer_pos must wake on moves"
    );

    // Next pass doesn't read → subscription lapses with the rest of
    // the per-pass set.
    ui.run_at(UVec2::new(200, 200), empty);
    let delta = ui.on_input(InputEvent::PointerMoved(Vec2::new(60.0, 50.0)));
    assert!(
        !delta.requests_repaint,
        "no read this pass → moves over an inert surface skip again"
    );
}

#[test]
fn pointer_local_read_keeps_hover_local_indicator_reactive() {
    fn indicator(ui: &mut Ui, id: WidgetId, painted_at: &mut Option<Vec2>) {
        Panel::canvas()
            .id(id)
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                let local = ui.pointer_local(id);
                if let Some(center) = local {
                    ui.add_shape(Shape::circle(center, 3.0, 2.0).brush(Color::rgb(0.2, 0.8, 1.0)));
                }
                *painted_at = local;
            });
    }

    let id = WidgetId::from_hash("pointer-local-indicator");
    let surface = UVec2::new(200, 200);
    let mut ui = Ui::for_test();
    ui.input_policy = InputPolicy::OnDelta;
    let mut painted_at = None;
    ui.run_at(surface, |ui| indicator(ui, id, &mut painted_at));

    let response = ui.response_for(id);
    let layout_rect = response.layout_rect.expect("indicator arranged");
    let origin = response.transform.apply_point(layout_rect.min);
    assert!(!response.hovered, "the indicator surface is inert");

    for expected in [Vec2::new(20.0, 25.0), Vec2::new(70.0, 60.0)] {
        let delta = ui.on_input(InputEvent::PointerMoved(origin + expected));
        assert!(
            delta.requests_repaint,
            "pointer-local paint must wake on movement within one inert surface",
        );
        ui.run_at(surface, |ui| indicator(ui, id, &mut painted_at));
        assert_eq!(painted_at, Some(expected));
    }
}

#[test]
fn modifiers_read_keeps_alt_ctrl_visual_reactive_through_release() {
    fn visual(ui: &mut Ui, painted: &mut Color) {
        let modifiers = ui.modifiers();
        let color = if modifiers.alt && modifiers.ctrl {
            Color::WHITE
        } else if modifiers.alt {
            Color::rgb(1.0, 0.0, 0.0)
        } else if modifiers.ctrl {
            Color::rgb(0.0, 0.0, 1.0)
        } else {
            Color::BLACK
        };
        *painted = color;
        Frame::new()
            .id(WidgetId::from_hash("modifier-visual"))
            .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
            .background(Background::fill(color))
            .show(ui);
    }

    let surface = UVec2::new(200, 200);
    let mut ui = Ui::for_test();
    ui.input_policy = InputPolicy::OnDelta;
    let mut painted = Color::TRANSPARENT;
    ui.run_at(surface, |ui| visual(ui, &mut painted));
    assert_eq!(painted, Color::BLACK);

    let states = [
        (
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Color::rgb(1.0, 0.0, 0.0),
        ),
        (
            Modifiers {
                alt: true,
                ctrl: true,
                ..Modifiers::NONE
            },
            Color::WHITE,
        ),
        (
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            Color::rgb(0.0, 0.0, 1.0),
        ),
        (Modifiers::NONE, Color::BLACK),
    ];
    for (modifiers, expected) in states {
        let delta = ui.on_input(InputEvent::ModifiersChanged(modifiers));
        assert!(
            delta.requests_repaint,
            "modifier-dependent paint must wake on every press and release",
        );
        ui.run_at(surface, |ui| visual(ui, &mut painted));
        assert_eq!(painted, expected);
    }
}

#[test]
fn key_chord_subscriber_wakes_only_exact_chord() {
    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(200, 200), empty_sub_escape);
    assert!(ui.input.focused.is_none());

    let delta = ui.on_input(InputEvent::KeyDown {
        key: Key::Enter,
        repeat: false,
        physical: Key::Other,
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
        physical: Key::Other,
    });
    assert!(!delta.requests_repaint);

    let _ = ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE));
    let delta = ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
        physical: Key::Other,
    });
    assert!(delta.requests_repaint);
}

#[test]
fn pointer_events_drain_between_frames() {
    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(200, 200), empty_sub_buttons);

    let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    let _ = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    assert_eq!(ui.pointer_events().len(), 1);

    ui.run_at(UVec2::new(200, 200), empty_sub_buttons);
    assert!(ui.pointer_events().is_empty());
}
