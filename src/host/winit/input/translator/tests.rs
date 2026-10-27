use glam::Vec2;
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

use crate::common::platform::{PLATFORM, Platform};
use crate::host::winit::input::translator::{PointerTrace, Translator};
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;

fn wheel(delta: MouseScrollDelta) -> WindowEvent {
    WindowEvent::MouseWheel {
        device_id: DeviceId::dummy(),
        delta,
        phase: TouchPhase::Moved,
    }
}

fn cursor_moved(x: f64, y: f64) -> WindowEvent {
    WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: PhysicalPosition::new(x, y),
    }
}

fn pinch(delta: f64) -> WindowEvent {
    WindowEvent::PinchGesture {
        device_id: DeviceId::dummy(),
        delta,
        phase: TouchPhase::Moved,
    }
}

#[test]
fn wheel_deltas_are_logical_and_point_in_scroll_direction() {
    let lines = wheel(MouseScrollDelta::LineDelta(2.0, 1.0));
    let pixels = wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
        60.0, -120.0,
    )));
    let mut got = Vec::new();
    Translator::new(PLATFORM).translate(&lines, 1.0, |event| got.push(event));
    assert!(matches!(
        got.as_slice(),
        [InputEvent::ScrollLines(delta)] if *delta == Vec2::new(-2.0, -1.0)
    ));
    got.clear();

    Translator::new(PLATFORM).translate(&pixels, 2.0, |event| got.push(event));
    assert!(matches!(
        got.as_slice(),
        [InputEvent::ScrollPixels(delta)] if *delta == Vec2::new(-30.0, 60.0)
    ));

    // A wheel event carries no modifiers: the translator keeps the last ones
    // reported, so Shift turns a later (0, -3) wheel, (0, 3) as a scroll,
    // into (3, 0) until its release arrives.
    let mut translator = Translator::new(Platform::Linux);
    let down = wheel(MouseScrollDelta::LineDelta(0.0, -3.0));
    for (modifiers, want) in [
        (ModifiersState::SHIFT, Vec2::new(3.0, 0.0)),
        (ModifiersState::empty(), Vec2::new(0.0, 3.0)),
    ] {
        translator.translate(
            &WindowEvent::ModifiersChanged(modifiers.into()),
            1.0,
            |_| {},
        );
        got.clear();
        translator.translate(&down, 1.0, |event| got.push(event));
        assert!(
            matches!(got.as_slice(), [InputEvent::ScrollLines(delta)] if *delta == want),
            "{modifiers:?}: {got:?}",
        );
    }
}

/// A pinch delta is a displacement, so the factor is `1 + delta`; this layer only converts and ingress rejects
/// factors no zoom can use (`InputEvent::is_valid`).
#[test]
fn pinch_translation_converts_and_leaves_the_screen_to_ingress() {
    let half = pinch(0.5);
    let mut emitted = Vec::new();
    Translator::new(PLATFORM).translate(&half, 1.0, |event| emitted.push(event));
    assert!(matches!(emitted.as_slice(), [InputEvent::Zoom(1.5)]));
    assert!(emitted[0].is_valid());

    for delta in [-1.0, -2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let event = pinch(delta);
        let mut emitted = Vec::new();
        Translator::new(PLATFORM).translate(&event, 1.0, |event| emitted.push(event));
        let [event] = emitted.as_slice() else {
            panic!("translation emits exactly one event, got {emitted:?}");
        };
        assert!(
            !event.is_valid(),
            "pinch delta {delta:?} produced a usable factor",
        );
    }
}

/// The recorder gets a logical position and the host the physical one it came from (what `Window::resync_pointer`
/// re-divides on scale changes); a sent move traces as the OS move it would come from.
#[test]
fn a_move_emits_logical_and_traces_physical() {
    let moved = cursor_moved(300.0, 120.0);
    let mut emitted = Vec::new();
    let trace = Translator::new(PLATFORM).translate(&moved, 2.5, |event| {
        emitted.push(event);
    });

    assert!(matches!(
        emitted.as_slice(),
        [InputEvent::PointerMoved(at)] if *at == Vec2::new(120.0, 48.0)
    ));
    assert_eq!(trace, PointerTrace::At(Vec2::new(300.0, 120.0)));
    assert_eq!(PointerTrace::of_input(emitted[0], 2.5), trace);
    assert_eq!(
        PointerTrace::of_input(InputEvent::PointerMoved(Vec2::new(f32::NAN, 1.0)), 2.5),
        PointerTrace::Unchanged,
        "a move the input screen refuses moves no anchor",
    );
}

/// A departure is traced as one, so the host drops a position it must not restate.
#[test]
fn a_departure_traces_gone_and_other_events_trace_nothing() {
    let left = WindowEvent::CursorLeft {
        device_id: DeviceId::dummy(),
    };
    let mut emitted = Vec::new();
    assert_eq!(
        Translator::new(PLATFORM).translate(&left, 1.0, |event| emitted.push(event)),
        PointerTrace::Gone,
    );
    assert!(matches!(emitted.as_slice(), [InputEvent::PointerLeft]));
    assert_eq!(
        PointerTrace::of_input(InputEvent::PointerLeft, 1.0),
        PointerTrace::Gone
    );
    assert_eq!(
        PointerTrace::of_input(InputEvent::PointerPressed(PointerButton::Left), 1.0),
        PointerTrace::Unchanged,
    );

    assert_eq!(
        Translator::new(PLATFORM).translate(&pinch(0.5), 1.0, |_| {}),
        PointerTrace::Unchanged,
        "a gesture says nothing about where the pointer is",
    );
    assert_eq!(
        Translator::new(PLATFORM).translate(
            &wheel(MouseScrollDelta::LineDelta(1.0, 1.0)),
            1.0,
            |_| {}
        ),
        PointerTrace::Unchanged,
    );
}

/// Each IME event as the crate's: preedit with ordered cursor, commit, disable as the empty preedit, enable as nothing.
#[test]
fn ime_events_translate_and_a_disable_ends_the_composition() {
    use winit::event::Ime;

    let rows = [
        (
            Ime::Preedit("かな".into(), Some((6, 3))),
            Some(
                "ImePreedit(ImePreedit { text: \"かな\", cursor: Some(Span { start: 3, len: 3 }) })",
            ),
        ),
        (
            Ime::Preedit("か".into(), None),
            Some("ImePreedit(ImePreedit { text: \"か\", cursor: None })"),
        ),
        (Ime::Commit("仮名".into()), Some("ImeCommit(\"仮名\")")),
        (
            Ime::Disabled,
            Some("ImePreedit(ImePreedit { text: \"\", cursor: None })"),
        ),
        (Ime::Enabled, None),
    ];
    for (ime, want) in rows {
        let event = WindowEvent::Ime(ime);
        let mut emitted = Vec::new();
        Translator::new(PLATFORM)
            .translate(&event, 1.0, |event| emitted.push(format!("{event:?}")));
        assert_eq!(emitted, want.into_iter().collect::<Vec<_>>(), "{event:?}");
    }
}

fn touch(id: u64, phase: TouchPhase, x: f64, y: f64) -> WindowEvent {
    WindowEvent::Touch(Touch {
        device_id: DeviceId::dummy(),
        phase,
        location: PhysicalPosition::new(x, y),
        force: None,
        id,
    })
}

fn mouse(state: ElementState, button: MouseButton) -> WindowEvent {
    WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state,
        button,
    }
}

/// Translates `event` at scale 2 into `emitted`, cleared first.
fn feed<'e>(
    translator: &mut Translator,
    event: &'e WindowEvent,
    emitted: &mut Vec<InputEvent<'e>>,
) -> PointerTrace {
    emitted.clear();
    translator.translate(event, 2.0, |event| emitted.push(event))
}

/// A tap is a click: one finger is the left button, pressed and released where it
/// touched, and leaves nothing hovered. While it is down, other fingers and the
/// mouse's position and left button change nothing.
#[test]
fn one_finger_drives_the_pointer_and_holds_it() {
    let down = touch(7, TouchPhase::Started, 200.0, 100.0);
    let ignored = [
        touch(8, TouchPhase::Started, 10.0, 10.0),
        touch(8, TouchPhase::Moved, 12.0, 10.0),
        touch(8, TouchPhase::Ended, 12.0, 10.0),
        touch(9, TouchPhase::Cancelled, 10.0, 10.0),
        cursor_moved(40.0, 40.0),
        WindowEvent::CursorLeft {
            device_id: DeviceId::dummy(),
        },
        mouse(ElementState::Pressed, MouseButton::Left),
        mouse(ElementState::Released, MouseButton::Left),
    ];
    let right = mouse(ElementState::Pressed, MouseButton::Right);
    let moved = touch(7, TouchPhase::Moved, 210.0, 100.0);
    let up = touch(7, TouchPhase::Ended, 210.0, 104.0);
    let mouse_back = cursor_moved(40.0, 40.0);
    let mut translator = Translator::new(PLATFORM);
    let mut emitted = Vec::new();

    // Physical (200, 100) at scale 2 is logical (100, 50).
    assert_eq!(
        feed(&mut translator, &down, &mut emitted),
        PointerTrace::At(Vec2::new(200.0, 100.0)),
    );
    assert!(matches!(
        emitted.as_slice(),
        [
            InputEvent::PointerMoved(p),
            InputEvent::PointerPressed(PointerButton::Left),
        ] if *p == Vec2::new(100.0, 50.0)
    ));

    for event in &ignored {
        assert_eq!(
            feed(&mut translator, event, &mut emitted),
            PointerTrace::Unchanged,
            "{event:?}",
        );
        assert!(emitted.is_empty(), "{event:?} emitted {emitted:?}");
    }
    assert_eq!(
        feed(&mut translator, &right, &mut emitted),
        PointerTrace::Unchanged,
    );
    assert!(
        matches!(
            emitted.as_slice(),
            [InputEvent::PointerPressed(PointerButton::Right)]
        ),
        "the right button is no part of the finger's press",
    );

    assert_eq!(
        feed(&mut translator, &moved, &mut emitted),
        PointerTrace::At(Vec2::new(210.0, 100.0)),
    );
    assert!(matches!(
        emitted.as_slice(),
        [InputEvent::PointerMoved(p)] if *p == Vec2::new(105.0, 50.0)
    ));
    assert_eq!(feed(&mut translator, &up, &mut emitted), PointerTrace::Gone);
    assert!(matches!(
        emitted.as_slice(),
        [
            InputEvent::PointerMoved(p),
            InputEvent::PointerReleased(PointerButton::Left),
            InputEvent::PointerLeft,
        ] if *p == Vec2::new(105.0, 52.0)
    ));

    assert_eq!(
        feed(&mut translator, &mouse_back, &mut emitted),
        PointerTrace::At(Vec2::new(40.0, 40.0)),
        "the lifted finger gives the pointer back to the mouse",
    );
    assert!(matches!(
        emitted.as_slice(),
        [InputEvent::PointerMoved(p)] if *p == Vec2::new(20.0, 20.0)
    ));
}

/// A cancelled touch leaves before it releases, so the release hit-tests nothing
/// and the press ends as a gesture cut off does: no click.
#[test]
fn a_cancelled_touch_leaves_before_it_releases() {
    let down = touch(1, TouchPhase::Started, 6.0, 4.0);
    let cancel = touch(1, TouchPhase::Cancelled, 6.0, 4.0);
    let next = touch(2, TouchPhase::Started, 8.0, 8.0);
    let mut translator = Translator::new(PLATFORM);
    let mut emitted = Vec::new();

    feed(&mut translator, &down, &mut emitted);
    assert_eq!(
        feed(&mut translator, &cancel, &mut emitted),
        PointerTrace::Gone
    );
    assert!(matches!(
        emitted.as_slice(),
        [
            InputEvent::PointerLeft,
            InputEvent::PointerReleased(PointerButton::Left),
        ]
    ));
    assert_eq!(
        feed(&mut translator, &next, &mut emitted),
        PointerTrace::At(Vec2::new(8.0, 8.0)),
        "the next finger takes the pointer",
    );
}

/// A lost end does not leave touch dead: a start from the held finger cancels
/// the old press and starts a new one, and focus loss forgets the finger, as the
/// recorder drops its press.
#[test]
fn a_lost_touch_end_does_not_hold_the_pointer() {
    let blur = WindowEvent::Focused(false);
    let down = touch(3, TouchPhase::Started, 20.0, 20.0);
    let restart = touch(3, TouchPhase::Started, 30.0, 30.0);
    let late_up = touch(3, TouchPhase::Ended, 30.0, 30.0);
    let other = touch(4, TouchPhase::Started, 50.0, 50.0);
    let mut translator = Translator::new(PLATFORM);
    let mut emitted = Vec::new();

    assert_eq!(
        feed(&mut translator, &blur, &mut emitted),
        PointerTrace::Unchanged,
        "with no finger down, focus loss says nothing about the pointer",
    );
    assert!(matches!(emitted.as_slice(), [InputEvent::SurfaceFocusLost]));

    feed(&mut translator, &down, &mut emitted);
    assert_eq!(
        feed(&mut translator, &restart, &mut emitted),
        PointerTrace::At(Vec2::new(30.0, 30.0)),
    );
    assert!(matches!(
        emitted.as_slice(),
        [
            InputEvent::PointerLeft,
            InputEvent::PointerReleased(PointerButton::Left),
            InputEvent::PointerMoved(p),
            InputEvent::PointerPressed(PointerButton::Left),
        ] if *p == Vec2::new(15.0, 15.0)
    ));

    assert_eq!(
        feed(&mut translator, &blur, &mut emitted),
        PointerTrace::Gone
    );
    assert!(matches!(
        emitted.as_slice(),
        [InputEvent::SurfaceFocusLost, InputEvent::PointerLeft]
    ));
    assert_eq!(
        feed(&mut translator, &late_up, &mut emitted),
        PointerTrace::Unchanged,
    );
    assert!(
        emitted.is_empty(),
        "the forgotten finger emitted {emitted:?}"
    );
    assert_eq!(
        feed(&mut translator, &other, &mut emitted),
        PointerTrace::At(Vec2::new(50.0, 50.0)),
        "another finger takes the pointer",
    );
}

/// A release names the logical key its press reported, by the physical key both came from:
/// `a` pressed, Shift in between, and its release is still `a`. A key this surface never saw go
/// down releases nothing; a second press of a held key ends the first with its lost release; and
/// focus loss forgets every held key, so a release after it says nothing.
#[test]
fn a_release_names_its_press_by_the_physical_key() {
    let a = PhysicalKey::Code(KeyCode::KeyA);
    let b = PhysicalKey::Code(KeyCode::KeyB);
    let mut translator = Translator::new(PLATFORM);
    let mut emitted = Vec::new();

    assert!(translator.hold(a, Key::Char('a')).is_none());
    assert!(matches!(
        translator.release(a),
        Some(InputEvent::KeyUp {
            key: Key::Char('a')
        })
    ));
    assert!(translator.release(a).is_none(), "released once");
    assert!(translator.release(b).is_none(), "never pressed here");

    assert!(translator.hold(a, Key::Char('a')).is_none());
    let lost = translator.hold(a, Key::Char('A'));
    assert!(
        matches!(
            lost,
            Some(InputEvent::KeyUp {
                key: Key::Char('a')
            })
        ),
        "{lost:?}"
    );
    assert!(matches!(
        translator.release(a),
        Some(InputEvent::KeyUp {
            key: Key::Char('A')
        })
    ));

    assert!(translator.hold(b, Key::Char('b')).is_none());
    feed(&mut translator, &WindowEvent::Focused(false), &mut emitted);
    assert!(translator.release(b).is_none(), "focus loss released it");
}
