//! Off-target wake gates (`PointerWake` flags and key chords): no watcher means
//! no wake and no `frame_pointer_events` entry; pre-record clear drops stale watches.
use crate::input::input_state::tests::{Stream, sample_layers};
use crate::primitives::identity::widget_id::WidgetId;

use crate::KeyFilter;
use crate::Ui;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::policy::InputPolicy;
use crate::input::shortcut::Shortcut;
use crate::input::watch::PointerWake;
use crate::internals::harness::UiHarness;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::modal::Modal;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

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
    assert!(delta.repaint_requested);

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
    assert!(!delta.repaint_requested);
    assert!(h.ui.pointer_events().is_empty());
}

#[test]
fn record_without_rewatch_drops_wake() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_buttons);
    h.frame(empty);

    h.move_to(Vec2::new(50.0, 50.0));
    let delta = h.press();
    assert!(!delta.repaint_requested);
}

#[test]
fn press_and_release_both_captured() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_buttons);

    h.press_at(Vec2::new(50.0, 50.0));
    let release = h.release();
    assert!(release.repaint_requested);

    let events = h.ui.pointer_events();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], PointerEvent::Down { .. }));
    assert!(matches!(events[1], PointerEvent::Up { .. }));
}

#[test]
fn move_watcher_wakes_on_inert_move() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(empty_watch_move);

    let delta = h.move_to(Vec2::new(50.0, 50.0));
    assert!(delta.repaint_requested);

    let events = h.ui.pointer_events();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        PointerEvent::Move(p) if p == Vec2::new(50.0, 50.0)
    ));
}

/// With `MOVE` unwatched there is no `Move` in the stream.
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

    assert!(delta.repaint_requested);
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

/// `SCROLL` and `PINCH` are separate wake categories; both directions are
/// tested so aliased bits fail.
#[test]
fn scroll_and_pinch_wake_categories_are_independent() {
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
            scroll.repaint_requested, *wants_scroll,
            "{label}: scroll wake"
        );
        assert_eq!(zoom.repaint_requested, *wants_zoom, "{label}: pinch wake");

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

/// Reading `Ui::pointer_pos` during record auto-asserts `MOVE`; a pass that
/// stops reading drops the wake.
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
        delta.repaint_requested,
        "a record pass that read pointer_pos must wake on moves"
    );

    h.frame(empty);
    let delta = h.move_to(Vec2::new(60.0, 50.0));
    assert!(
        !delta.repaint_requested,
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
                    ui.add_shape(Shape::circle(
                        center,
                        3.0,
                        Stroke::new(RgbaF32::srgb(0.2, 0.8, 1.0), 2.0),
                    ));
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
    assert!(!response.hovered(), "the indicator surface is inert");

    for expected in [Vec2::new(20.0, 25.0), Vec2::new(70.0, 60.0)] {
        let delta = h.move_to(origin + expected);
        assert!(
            delta.repaint_requested,
            "pointer-local paint must wake on movement within one inert surface",
        );
        h.frame(|ui| indicator(ui, id, &mut painted_at));
        assert_eq!(painted_at, Some(expected));
    }
}

#[test]
fn modifiers_read_keeps_alt_ctrl_visual_reactive_through_release() {
    fn visual(ui: &mut Ui, painted: &mut RgbaF32) {
        let modifiers = ui.modifiers();
        let color = if modifiers.alt && modifiers.ctrl {
            RgbaF32::WHITE
        } else if modifiers.alt {
            RgbaF32::srgb(1.0, 0.0, 0.0)
        } else if modifiers.ctrl {
            RgbaF32::srgb(0.0, 0.0, 1.0)
        } else {
            RgbaF32::BLACK
        };
        *painted = color;
        Block::new()
            .id(WidgetId::from_hash("modifier-visual"))
            .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
            .background(Background::fill(color))
            .show(ui);
    }

    let surface = UVec2::new(200, 200);
    let mut h = UiHarness::new(surface);
    h.ui.set_input_policy(InputPolicy::OnDelta);
    let mut painted = RgbaF32::TRANSPARENT;
    h.frame(|ui| visual(ui, &mut painted));
    assert_eq!(painted, RgbaF32::BLACK);

    let states = [
        (Modifiers::ALT, RgbaF32::srgb(1.0, 0.0, 0.0)),
        (
            Modifiers {
                alt: true,
                ctrl: true,
                ..Modifiers::NONE
            },
            RgbaF32::WHITE,
        ),
        (Modifiers::CTRL, RgbaF32::srgb(0.0, 0.0, 1.0)),
        (Modifiers::NONE, RgbaF32::BLACK),
    ];
    for (modifiers, expected) in states {
        let delta = h.on_input(InputEvent::ModifiersChanged(modifiers));
        assert!(
            delta.repaint_requested,
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
    assert_eq!(h.focus(), None);

    let delta = h.key(Key::Enter);
    assert!(!delta.repaint_requested);

    // Alt+Escape against a bare-Escape watcher: no match (not ctrl, which matches on macOS).
    let alt = Modifiers::ALT;
    h.set_modifiers(alt);
    let delta = h.key(Key::Escape);
    assert!(!delta.repaint_requested);

    h.set_modifiers(Modifiers::NONE);
    let delta = h.key(Key::Escape);
    assert!(delta.repaint_requested);

    h.frame(|ui| {
        empty_watch_escape(ui);
        ui.watch_key(Shortcut::key(Key::Escape));
    });
    let delta = h.key(Key::Enter);
    assert!(!delta.repaint_requested);
    let delta = h.key(Key::Escape);
    assert!(delta.repaint_requested);
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

/// The pointer watch stream is layer-gated like the keyboard's: an overlay's
/// scope silences watchers strictly below it.
#[test]
fn a_scope_silences_pointer_watchers_strictly_below_it() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let scoped = |ui: &mut Ui| {
        empty_watch_buttons(ui);
        ui.layer(Layer::Popup).show(|ui| {
            Block::new()
                .id(WidgetId::from_hash("overlay"))
                .input_scope(KeyFilter::ALL)
                .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                .show(ui);
        });
    };
    // Two frames: the scope records, then resolves. Counts are read inside pass A.
    h.frame(scoped);
    h.press_at(Vec2::new(50.0, 50.0));
    let seen = sample_layers(&mut h, Stream::Pointer, scoped).layers;

    assert_eq!(seen[Layer::Main.idx()], 0);
    assert_eq!(seen[Layer::Popup.idx()], 1);
    assert_eq!(seen[Layer::Modal.idx()], 1);
    assert_eq!(seen[Layer::Tooltip.idx()], 1);

    h.release();
    h.frame(empty_watch_buttons);
    h.press_at(Vec2::new(50.0, 50.0));
    assert_eq!(
        sample_layers(&mut h, Stream::Pointer, empty_watch_buttons).layers[Layer::Main.idx()],
        1,
    );
}

/// End to end: a `Modal` takes the stream from `Main`, a plain `Ui::layer` on the same layer does not.
#[test]
fn only_a_scope_gates_the_stream_and_only_while_recorded() {
    let surface = UVec2::new(200, 200);
    let press_point = Vec2::new(50.0, 50.0);
    let with_modal = |ui: &mut Ui| {
        empty_watch_buttons(ui);
        Modal::new().show(ui, |_, _| {});
    };
    let plain_layer = |ui: &mut Ui| {
        empty_watch_buttons(ui);
        ui.layer(Layer::Modal).show(empty);
    };

    // A scope takes effect the frame after it is declared, so each leg records twice.
    let mut h = UiHarness::new(surface);
    h.frame(with_modal);
    h.press_at(press_point);
    assert_eq!(
        sample_layers(&mut h, Stream::Pointer, with_modal).layers[Layer::Main.idx()],
        0,
        "a Modal declares an ALL scope on its layer, so Main is cut off",
    );

    h.release();
    h.frame(plain_layer);
    h.press_at(press_point);
    assert_eq!(
        sample_layers(&mut h, Stream::Pointer, plain_layer).layers[Layer::Main.idx()],
        1,
        "a plain layer on the same Layer::Modal declares nothing and blocks nothing",
    );

    h.release();
    h.frame(empty_watch_buttons);
    h.press_at(press_point);
    assert_eq!(
        sample_layers(&mut h, Stream::Pointer, empty_watch_buttons).layers[Layer::Main.idx()],
        1,
        "a modal that stops recording stops blocking",
    );
}

/// `peek_*` reads the same value as its watching twin without the wake.
#[test]
fn peeks_return_the_watched_value_without_asserting_the_watch() {
    let surface = UVec2::new(200, 200);
    let id = WidgetId::from_hash("root");
    let at = Vec2::new(50.0, 50.0);

    let mut watched = UiHarness::new(surface);
    watched.frame(|ui| {
        empty(ui);
        let _ = ui.pointer_pos();
        let _ = ui.modifiers();
    });
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
            .repaint_requested,
        "pointer_pos watches MOVE",
    );
    assert!(
        !peeked
            .on_input(InputEvent::PointerMoved(Vec2::new(60.0, 50.0)))
            .repaint_requested,
        "peek_pointer_pos must not",
    );

    let mods = Modifiers::SHIFT;
    assert!(
        watched
            .on_input(InputEvent::ModifiersChanged(mods))
            .repaint_requested,
        "modifiers watches MODIFIER",
    );
    assert!(
        !peeked
            .on_input(InputEvent::ModifiersChanged(mods))
            .repaint_requested,
        "peek_modifiers must not",
    );

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
