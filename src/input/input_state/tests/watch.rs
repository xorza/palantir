//! Off-target wake gates — `PointerWake` flags + specific key
//! chords. Pinned axes:
//!  * watcher wakes the frame on events that otherwise wouldn't
//!    (press on inert surface, key with no focus);
//!  * no watcher → no wake AND no entry in `frame_pointer_events`
//!    (the `any_mask` short-circuit gates the push);
//!  * pre-record clear drops stale watches.
use crate::primitives::widget_id::WidgetId;

use crate::KeyFilter;
use crate::Ui;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::policy::InputPolicy;
use crate::input::shortcut::Shortcut;
use crate::input::watch::PointerWake;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::shape::Shape;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::modal::Modal;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};
use strum::EnumCount as _;

fn empty(ui: &mut Ui) {
    Panel::vstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |_| {});
}

fn empty_watch_buttons(ui: &mut Ui) {
    empty(ui);
    ui.watch_pointer(PointerWake::BUTTONS);
}

fn empty_watch_move(ui: &mut Ui) {
    empty(ui);
    ui.watch_pointer(PointerWake::MOVE);
}

fn empty_watch_escape(ui: &mut Ui) {
    empty(ui);
    ui.watch_key(Shortcut::key(Key::Escape));
}

#[test]
fn buttons_watcher_wakes_press_on_inert() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_buttons);

    h.move_to(Vec2::new(50.0, 50.0));
    let delta = h.press();
    assert!(delta.requests_repaint);

    let events = h.ui.pointer_events();
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
fn press_on_inert_with_no_watcher_does_not_wake() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty);
    h.move_to(Vec2::new(50.0, 50.0));
    let delta = h.press();
    assert!(!delta.requests_repaint);
    assert!(h.ui.pointer_events().is_empty());
}

#[test]
fn record_without_rewatch_drops_wake() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_buttons);
    h.frame(empty);

    h.move_to(Vec2::new(50.0, 50.0));
    let delta = h.press();
    assert!(!delta.requests_repaint);
}

#[test]
fn press_and_release_both_captured() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_buttons);

    h.press_at(Vec2::new(50.0, 50.0));
    let release = h.release();
    assert!(release.requests_repaint);

    let events = h.ui.pointer_events();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], PointerEvent::Down { .. }));
    assert!(matches!(events[1], PointerEvent::Up { .. }));
}

/// `MOVE` wakes on every pointer move — even inert ones.
#[test]
fn move_watcher_wakes_on_inert_move() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_move);

    let delta = h.move_to(Vec2::new(50.0, 50.0));
    assert!(delta.requests_repaint);

    let events = h.ui.pointer_events();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        PointerEvent::Move(p) if p == Vec2::new(50.0, 50.0)
    ));
}

/// `MOVE` not watched → no `Move` in the stream even
/// though hover may have changed. Hover-driven wake still fires
/// via the existing hit-test path; we're only checking the buffer.
#[test]
fn move_without_watcher_does_not_log() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty);
    h.move_to(Vec2::new(50.0, 50.0));
    assert!(h.ui.pointer_events().is_empty());
}

#[test]
fn scroll_watcher_receives_an_event_without_creating_a_widget_delta() {
    fn empty_watch_scroll(ui: &mut Ui) {
        empty(ui);
        ui.watch_pointer(PointerWake::SCROLL);
    }

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_scroll);
    let delta = h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 7.0));

    assert!(delta.requests_repaint);
    assert!(h.ui.input().frame_target_deltas.is_empty());
    assert!(matches!(
        h.ui.pointer_events(),
        [PointerEvent::Scroll {
            pos,
            pixels,
            lines,
        }] if *pos == Vec2::new(50.0, 50.0)
            && *pixels == Vec2::new(0.0, 7.0)
            && *lines == Vec2::ZERO
    ));
}

/// `SCROLL` and `PINCH` are separate wake categories, mirroring the
/// `Sense` split — a wheel tick and a touchpad pinch are different
/// gestures with different routing targets, and watching one must not
/// wake on the other. Both directions, because a one-sided assertion
/// would still pass if the two bits were aliased.
#[test]
fn scroll_and_pinch_wake_categories_are_independent() {
    // (label, watched category, expects scroll, expects zoom)
    let cases: &[(&str, PointerWake, bool, bool)] = &[
        ("scroll only", PointerWake::SCROLL, true, false),
        ("pinch only", PointerWake::PINCH, false, true),
        (
            "both",
            PointerWake::SCROLL.union(PointerWake::PINCH),
            true,
            true,
        ),
    ];

    for (label, watched, wants_scroll, wants_zoom) in cases {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.frame(|ui| {
            empty(ui);
            ui.watch_pointer(*watched);
        });
        let scroll = h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 7.0));
        let zoom = h.pinch(1.25);

        assert_eq!(
            scroll.requests_repaint, *wants_scroll,
            "{label}: scroll wake"
        );
        assert_eq!(zoom.requests_repaint, *wants_zoom, "{label}: pinch wake");

        let scrolls =
            h.ui.pointer_events()
                .iter()
                .filter(|e| matches!(e, PointerEvent::Scroll { .. }))
                .count();
        let zooms =
            h.ui.pointer_events()
                .iter()
                .filter(|e| matches!(e, PointerEvent::Zoom { .. }))
                .count();
        assert_eq!(
            scrolls,
            usize::from(*wants_scroll),
            "{label}: scroll stream"
        );
        assert_eq!(zooms, usize::from(*wants_zoom), "{label}: pinch stream");
    }
}

/// Reading `Ui::pointer_pos` during record auto-asserts `MOVE`: record
/// output derived from the raw pointer may change on any move, so moves
/// must wake even over an inert surface. A pass that stops reading
/// drops the wake like any other lapsed watch — the staleness
/// this pins: a pointer-proximity highlight painted from `pointer_pos`
/// must not freeze on screen when the hover target stops changing.
#[test]
fn pointer_pos_read_asserts_move_watch() {
    fn empty_reads_pointer(ui: &mut Ui) {
        empty(ui);
        let _ = ui.pointer_pos();
    }

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_reads_pointer);
    let delta = h.move_to(Vec2::new(50.0, 50.0));
    assert!(
        delta.requests_repaint,
        "a record pass that read pointer_pos must wake on moves"
    );

    // Next pass doesn't read → watch lapses with the rest of
    // the per-pass set.
    h.frame(empty);
    let delta = h.move_to(Vec2::new(60.0, 50.0));
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
    let mut h = UiHarness::new(surface);
    h.ui.set_input_policy(InputPolicy::OnDelta);
    let mut painted_at = None;
    h.frame(|ui| indicator(ui, id, &mut painted_at));

    let response = h.ui.response_for(id);
    let layout_rect = response.layout_rect.expect("indicator arranged");
    let origin = response.transform.apply_point(layout_rect.min);
    assert!(!response.hovered, "the indicator surface is inert");

    for expected in [Vec2::new(20.0, 25.0), Vec2::new(70.0, 60.0)] {
        let delta = h.move_to(origin + expected);
        assert!(
            delta.requests_repaint,
            "pointer-local paint must wake on movement within one inert surface",
        );
        h.frame(|ui| indicator(ui, id, &mut painted_at));
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
    let mut h = UiHarness::new(surface);
    h.ui.set_input_policy(InputPolicy::OnDelta);
    let mut painted = Color::TRANSPARENT;
    h.frame(|ui| visual(ui, &mut painted));
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
        // Raw, not `set_modifiers`: this asserts the modifier wake, and
        // the helper's change-only emit has no delta to hand back.
        let delta = h.on_input(InputEvent::ModifiersChanged(modifiers));
        assert!(
            delta.requests_repaint,
            "modifier-dependent paint must wake on every press and release",
        );
        h.frame(|ui| visual(ui, &mut painted));
        assert_eq!(painted, expected);
    }
}

#[test]
fn key_chord_watcher_wakes_only_exact_chord() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_escape);
    assert!(h.ui.input().focused.is_none());

    let delta = h.key(Key::Enter);
    assert!(!delta.requests_repaint);

    // Alt+Escape: watcher asked for bare Escape → no match.
    // (Avoid ctrl here: on macOS, raw Ctrl isn't represented in
    // `Shortcut`'s `Mods` vocabulary, so ctrl+Escape would *match*
    // Shortcut::key(Escape) — a documented platform compromise.)
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    h.set_modifiers(alt);
    let delta = h.key(Key::Escape);
    assert!(!delta.requests_repaint);

    h.set_modifiers(Modifiers::NONE);
    let delta = h.key(Key::Escape);
    assert!(delta.requests_repaint);
}

#[test]
fn pointer_events_drain_between_frames() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_buttons);

    h.press_at(Vec2::new(50.0, 50.0));
    assert_eq!(h.ui.pointer_events().len(), 1);

    h.frame(empty_watch_buttons);
    assert!(h.ui.pointer_events().is_empty());
}

/// The pointer watch stream is layer-gated exactly like the keyboard's:
/// an overlay's scope silences watchers *strictly below* it and nobody
/// else. Same predicate, same shape as
/// `a_scope_silences_the_layers_strictly_below_it`.
#[test]
fn a_scope_silences_pointer_watchers_strictly_below_it() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let scoped = |ui: &mut Ui| {
        empty_watch_buttons(ui);
        ui.layer(Layer::Popup).show(|ui| {
            Frame::new()
                .id(WidgetId::from_hash("overlay"))
                .input_scope(KeyFilter::ALL)
                .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                .show(ui);
        });
    };
    // Two frames: the scope records in the first, resolves in the next.
    // Counts are read *inside* the record, the only place the queue is
    // live, and maxed across the double-layout passes.
    h.frame(scoped);
    h.press_at(Vec2::new(50.0, 50.0));
    let seen = sample_pointer_layers(&mut h, scoped);

    // Strictly below — cut off, which is the whole point.
    assert_eq!(seen[Layer::Main.idx()], 0);
    // Same layer — the overlay's own body keeps watching, so a popup can
    // still drive a drag inside itself.
    assert_eq!(seen[Layer::Popup.idx()], 1);
    // Above — a modal over a popup is not silenced by it.
    assert_eq!(seen[Layer::Modal.idx()], 1);
    assert_eq!(seen[Layer::Tooltip.idx()], 1);

    // Nothing re-declares it, so the next resolution reopens the stream.
    h.frame(empty_watch_buttons);
    h.press_at(Vec2::new(50.0, 50.0));
    assert_eq!(
        sample_pointer_layers(&mut h, empty_watch_buttons)[Layer::Main.idx()],
        1,
    );
}

/// Per-layer pointer-watch counts, the sibling of `sample_layers` in
/// `input::tests::keyboard` — same reason for reading inside the record.
fn sample_pointer_layers(
    h: &mut UiHarness,
    mut record: impl FnMut(&mut Ui),
) -> [usize; Layer::COUNT] {
    let mut seen = [0usize; Layer::COUNT];
    h.frame(|ui| {
        record(ui);
        for layer in Layer::PAINT_ORDER {
            let n = ui.input().pointer_events(layer).len();
            seen[layer.idx()] = seen[layer.idx()].max(n);
        }
    });
    seen
}

/// End-to-end, and the distinction an overlay's scope exists to draw: a
/// `Modal` takes the stream from `Main`, a plain `Ui::layer` on the very
/// same layer does not. Not recording the modal is the whole release.
#[test]
fn only_a_scope_gates_the_stream_and_only_while_recorded() {
    let surface = UVec2::new(200, 200);
    let press = |ui: &mut Ui| {
        let _ = ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        let _ = ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    };
    let with_modal = |ui: &mut Ui| {
        empty_watch_buttons(ui);
        Modal::new().show(ui, |_, _| {});
    };
    let plain_layer = |ui: &mut Ui| {
        empty_watch_buttons(ui);
        ui.layer(Layer::Modal).show(empty);
    };

    // A scope takes effect on the frame *after* the one that declared
    // it — the path resolves at record-pass start from the cascade the
    // previous frame left — so each leg records twice.
    let mut h = UiHarness::new(surface);
    h.frame(with_modal);
    press(&mut h.ui);
    assert_eq!(
        sample_pointer_layers(&mut h, with_modal)[Layer::Main.idx()],
        0,
        "a Modal declares an ALL scope on its layer, so Main is cut off",
    );

    // A paint-only overlay on the same layer — a tooltip, a debug HUD —
    // must leave the canvas underneath able to pan and zoom.
    h.frame(plain_layer);
    press(&mut h.ui);
    assert_eq!(
        sample_pointer_layers(&mut h, plain_layer)[Layer::Main.idx()],
        1,
        "a plain layer on the same Layer::Modal declares nothing and blocks nothing",
    );

    h.frame(empty_watch_buttons);
    press(&mut h.ui);
    assert_eq!(
        sample_pointer_layers(&mut h, empty_watch_buttons)[Layer::Main.idx()],
        1,
        "a modal that stops recording stops blocking",
    );
}

/// `peek_*` is the same value as its watching twin and none of the wake:
/// A → wakes, B → doesn't, and both report the same reading. The point
/// is the pair, so they're asserted against each other rather than
/// separately.
#[test]
fn peeks_return_the_watched_value_without_asserting_the_watch() {
    let surface = UVec2::new(200, 200);
    let id = WidgetId::from_hash("root");
    let at = Vec2::new(50.0, 50.0);

    // Watching pass: reads pointer + modifiers the auto-watch way.
    let mut watched = UiHarness::new(surface);
    watched.frame(|ui| {
        empty(ui);
        let _ = ui.pointer_pos();
        let _ = ui.modifiers();
    });
    // Peeking pass: same two reads, no watch.
    let mut peeked = UiHarness::new(surface);
    peeked.frame(|ui| {
        empty(ui);
        let _ = ui.peek_pointer_pos();
        let _ = ui.peek_modifiers();
    });

    for ui in [&mut watched, &mut peeked] {
        ui.move_to(at);
    }
    assert!(
        watched
            .on_input(InputEvent::PointerMoved(Vec2::new(60.0, 50.0)))
            .requests_repaint,
        "pointer_pos watches MOVE",
    );
    assert!(
        !peeked
            .on_input(InputEvent::PointerMoved(Vec2::new(60.0, 50.0)))
            .requests_repaint,
        "peek_pointer_pos must not",
    );

    let mods = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    assert!(
        watched
            .on_input(InputEvent::ModifiersChanged(mods))
            .requests_repaint,
        "modifiers watches MODIFIER",
    );
    assert!(
        !peeked
            .on_input(InputEvent::ModifiersChanged(mods))
            .requests_repaint,
        "peek_modifiers must not",
    );

    // Same reading from both, so the cheap one isn't cheap by lying.
    assert_eq!(peeked.ui.peek_pointer_pos(), Some(Vec2::new(60.0, 50.0)));
    assert_eq!(peeked.ui.peek_pointer_pos(), watched.ui.peek_pointer_pos());
    assert!(peeked.ui.peek_modifiers().shift);
    assert_eq!(peeked.ui.peek_modifiers(), watched.ui.peek_modifiers());
    assert_eq!(
        peeked.ui.peek_pointer_local(id),
        watched.ui.peek_pointer_local(id)
    );
    assert_eq!(
        peeked.ui.peek_pointer_local(id),
        Some(Vec2::new(60.0, 50.0))
    );
}
