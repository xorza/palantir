use glam::Vec2;
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::keyboard::{
    Key as WinitKey, KeyCode, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey,
};

use crate::common::platform::{PLATFORM, Platform};
use crate::host::winit::input::{
    KeyDownFacts, PointerTrace, Translation, key_down, logical_key, normalize_modifiers,
    physical_key, shift_wheel, translate,
};
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;

fn at(scale_factor: f32) -> Translation {
    Translation {
        scale_factor,
        modifiers: ModifiersState::empty(),
        platform: PLATFORM,
    }
}

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
fn logical_keys_map_to_native_vocabulary() {
    use NamedKey::*;
    let cases: &[(NamedKey, Key)] = &[
        (ArrowLeft, Key::ArrowLeft),
        (ArrowRight, Key::ArrowRight),
        (ArrowUp, Key::ArrowUp),
        (ArrowDown, Key::ArrowDown),
        (Backspace, Key::Backspace),
        (Delete, Key::Delete),
        (Home, Key::Home),
        (End, Key::End),
        (Enter, Key::Enter),
        (Escape, Key::Escape),
        (PageUp, Key::PageUp),
        (PageDown, Key::PageDown),
        (Tab, Key::Tab),
        (Space, Key::Char(' ')),
        (F24, Key::Other),
    ];
    for (named, expected) in cases {
        assert_eq!(logical_key(&WinitKey::Named(*named)), *expected);
    }
    assert_eq!(
        logical_key(&WinitKey::Character("A".into())),
        Key::Char('A')
    );
    assert_eq!(
        logical_key(&WinitKey::Character("é".into())),
        Key::Char('é')
    );
}

#[test]
fn physical_keys_map_layout_independent_identities() {
    let code = |code| physical_key(PhysicalKey::Code(code));
    assert_eq!(code(KeyCode::KeyA), Key::Char('a'));
    assert_eq!(code(KeyCode::KeyM), Key::Char('m'));
    assert_eq!(code(KeyCode::KeyZ), Key::Char('z'));
    assert_eq!(code(KeyCode::Digit0), Key::Char('0'));
    assert_eq!(code(KeyCode::Digit9), Key::Char('9'));
    assert_eq!(code(KeyCode::Enter), Key::Enter);
    assert_eq!(code(KeyCode::ArrowLeft), Key::ArrowLeft);
    assert_eq!(code(KeyCode::F1), Key::F1);
    assert_eq!(code(KeyCode::Insert), Key::Other);
    assert_eq!(
        physical_key(PhysicalKey::Unidentified(NativeKeyCode::Unidentified)),
        Key::Other
    );
}

/// Each winit modifier on each platform: on macOS Cmd (Super) is the
/// primary command bit and raw Control is `mac_ctrl`; elsewhere Control
/// is the command bit and Super is `meta`.
#[test]
fn modifier_normalization_translates_each_bit() {
    let none = Modifiers::NONE;
    let rows: [(ModifiersState, Platform, Modifiers); 8] = [
        (ModifiersState::empty(), Platform::Mac, none),
        (ModifiersState::SHIFT, Platform::Linux, Modifiers::SHIFT),
        (ModifiersState::ALT, Platform::Mac, Modifiers::ALT),
        (ModifiersState::SUPER, Platform::Mac, Modifiers::CTRL),
        (
            ModifiersState::CONTROL,
            Platform::Mac,
            Modifiers {
                mac_ctrl: true,
                ..none
            },
        ),
        (ModifiersState::CONTROL, Platform::Windows, Modifiers::CTRL),
        (
            ModifiersState::SUPER,
            Platform::Linux,
            Modifiers { meta: true, ..none },
        ),
        (
            ModifiersState::SHIFT | ModifiersState::CONTROL,
            Platform::Linux,
            Modifiers::CTRL_SHIFT,
        ),
    ];
    for (state, platform, expected) in rows {
        assert_eq!(
            normalize_modifiers(state, platform),
            expected,
            "{state:?} on {platform:?}"
        );
    }
}

#[test]
fn wheel_deltas_are_logical_and_point_in_scroll_direction() {
    let lines = wheel(MouseScrollDelta::LineDelta(2.0, 1.0));
    let pixels = wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
        60.0, -120.0,
    )));
    let mut got = Vec::new();
    translate(&lines, at(1.0), |event| got.push(event));
    assert!(matches!(
        got.as_slice(),
        [InputEvent::ScrollLines(delta)] if *delta == Vec2::new(-2.0, -1.0)
    ));
    got.clear();

    translate(&pixels, at(2.0), |event| got.push(event));
    assert!(matches!(
        got.as_slice(),
        [InputEvent::ScrollPixels(delta)] if *delta == Vec2::new(-30.0, 60.0)
    ));
}

/// A pinch delta is a *displacement*, so the factor is `1 + delta`. This
/// layer converts and nothing more — a delta of -1 or worse produces a
/// factor no zoom can use, and refusing it is `InputEvent::is_valid`'s
/// question, asked once at the ingress every other payload here goes
/// through.
#[test]
fn pinch_translation_converts_and_leaves_the_screen_to_ingress() {
    let half = pinch(0.5);
    let mut emitted = Vec::new();
    translate(&half, at(1.0), |event| emitted.push(event));
    assert!(matches!(emitted.as_slice(), [InputEvent::Zoom(1.5)]));
    assert!(emitted[0].is_valid());

    for delta in [-1.0, -2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let event = pinch(delta);
        let mut emitted = Vec::new();
        translate(&event, at(1.0), |event| emitted.push(event));
        let [event] = emitted.as_slice() else {
            panic!("translation emits exactly one event, got {emitted:?}");
        };
        assert!(
            !event.is_valid(),
            "pinch delta {delta:?} produced a usable factor",
        );
    }
}

/// The logical and physical tables must denote the same [`Key`] for
/// every key winit names in both vocabularies.
///
/// `Shortcut::matches`'s non-Latin fallback consults `physical` alone,
/// so a key that resolves on one side and not the other stops matching
/// under a non-Latin layout — silently, and only for users of that
/// layout. `shared_key!` makes them agree by construction; this pins it
/// against someone hand-adding an arm to one side instead.
#[test]
fn shared_keys_denote_the_same_key_on_both_sides() {
    // Every name `shared_key!` lists — a subset would not catch an
    // arm hand-added to one side outside the macro, which is the whole
    // failure this guards.
    let cases: &[(NamedKey, KeyCode)] = &[
        (NamedKey::ArrowLeft, KeyCode::ArrowLeft),
        (NamedKey::ArrowRight, KeyCode::ArrowRight),
        (NamedKey::ArrowUp, KeyCode::ArrowUp),
        (NamedKey::ArrowDown, KeyCode::ArrowDown),
        (NamedKey::Backspace, KeyCode::Backspace),
        (NamedKey::Delete, KeyCode::Delete),
        (NamedKey::Home, KeyCode::Home),
        (NamedKey::End, KeyCode::End),
        (NamedKey::PageUp, KeyCode::PageUp),
        (NamedKey::PageDown, KeyCode::PageDown),
        (NamedKey::Enter, KeyCode::Enter),
        (NamedKey::Tab, KeyCode::Tab),
        (NamedKey::Escape, KeyCode::Escape),
        (NamedKey::Space, KeyCode::Space),
        (NamedKey::F1, KeyCode::F1),
        (NamedKey::F2, KeyCode::F2),
        (NamedKey::F3, KeyCode::F3),
        (NamedKey::F4, KeyCode::F4),
        (NamedKey::F5, KeyCode::F5),
        (NamedKey::F6, KeyCode::F6),
        (NamedKey::F7, KeyCode::F7),
        (NamedKey::F8, KeyCode::F8),
        (NamedKey::F9, KeyCode::F9),
        (NamedKey::F10, KeyCode::F10),
        (NamedKey::F11, KeyCode::F11),
        (NamedKey::F12, KeyCode::F12),
    ];
    for &(named, code) in cases {
        let from_logical = logical_key(&WinitKey::Named(named));
        let from_physical = physical_key(PhysicalKey::Code(code));
        assert_eq!(
            from_logical, from_physical,
            "{named:?} / {code:?} must denote one Key",
        );
        assert_ne!(
            from_logical,
            Key::Other,
            "{named:?} must resolve on both sides, not fall through",
        );
    }
}

/// The recorder is told a logical position, and the host is handed back
/// the physical one it came from — the number `Window::resync_pointer`
/// re-divides when the scale moves, and the only pointer fact the
/// division would otherwise destroy.
#[test]
fn a_move_emits_logical_and_traces_physical() {
    let moved = cursor_moved(300.0, 120.0);
    let mut emitted = Vec::new();
    let trace = translate(&moved, at(2.5), |event| {
        emitted.push(event);
    });

    assert!(matches!(
        emitted.as_slice(),
        [InputEvent::PointerMoved(at)] if *at == Vec2::new(120.0, 48.0)
    ));
    assert_eq!(trace, PointerTrace::At(Vec2::new(300.0, 120.0)));
}

/// A departure is traced as one, so the host drops a position it must
/// not restate, and every other event leaves the trace alone.
#[test]
fn a_departure_traces_gone_and_other_events_trace_nothing() {
    let left = WindowEvent::CursorLeft {
        device_id: DeviceId::dummy(),
    };
    let mut emitted = Vec::new();
    assert_eq!(
        translate(&left, at(1.0), |event| emitted.push(event)),
        PointerTrace::Gone,
    );
    assert!(matches!(emitted.as_slice(), [InputEvent::PointerLeft]));

    assert_eq!(
        translate(&pinch(0.5), at(1.0), |_| {}),
        PointerTrace::Unchanged,
        "a gesture says nothing about where the pointer is",
    );
    assert_eq!(
        translate(
            &wheel(MouseScrollDelta::LineDelta(1.0, 1.0)),
            at(1.0),
            |_| {}
        ),
        PointerTrace::Unchanged,
    );
}

/// A synthetic press — winit's replay of a key still held when the
/// window gains focus — is not input. A real press with the same key
/// and text is.
#[test]
fn a_synthetic_press_is_dropped() {
    let logical = WinitKey::Named(NamedKey::Enter);
    let physical = PhysicalKey::Code(KeyCode::Enter);
    let facts = |is_synthetic| KeyDownFacts {
        logical: &logical,
        physical: &physical,
        text: Some("\r"),
        repeat: false,
        is_synthetic,
    };
    assert!(key_down(facts(true), ModifiersState::empty()).is_none());
    assert!(matches!(
        key_down(facts(false), ModifiersState::empty()),
        Some(InputEvent::KeyDown {
            key: Key::Enter,
            ..
        })
    ));
}

/// Super is a command modifier everywhere and has no `Modifiers` bit
/// off macOS, so text typed under it is cleared here. Without Super the
/// same press keeps its text.
#[test]
fn text_under_super_is_cleared() {
    let logical = WinitKey::Character("l".into());
    let physical = PhysicalKey::Code(KeyCode::KeyL);
    let facts = KeyDownFacts {
        logical: &logical,
        physical: &physical,
        text: Some("l"),
        repeat: false,
        is_synthetic: false,
    };
    let text_of = |modifiers| match key_down(facts, modifiers) {
        Some(InputEvent::KeyDown { text, .. }) => text,
        other => panic!("a press translates to a KeyDown, got {other:?}"),
    };
    assert!(text_of(ModifiersState::SUPER).is_empty());
    assert_eq!(text_of(ModifiersState::empty()).as_str(), "l");
    assert_eq!(text_of(ModifiersState::SHIFT).as_str(), "l");
}

/// Shift+wheel scrolls sideways on Windows and Linux, and only a purely
/// vertical delta is moved; macOS sends the horizontal delta itself.
#[test]
fn shift_wheel_turns_vertical_into_horizontal_off_macos() {
    let down = Vec2::new(0.0, 3.0);
    let diagonal = Vec2::new(1.0, 3.0);
    let shift = ModifiersState::SHIFT;
    let rows = [
        (down, shift, Platform::Linux, Vec2::new(3.0, 0.0)),
        (down, shift, Platform::Windows, Vec2::new(3.0, 0.0)),
        (down, shift, Platform::Mac, down),
        (down, ModifiersState::empty(), Platform::Linux, down),
        (diagonal, shift, Platform::Linux, diagonal),
    ];
    for (delta, modifiers, platform, expected) in rows {
        assert_eq!(
            shift_wheel(delta, modifiers, platform),
            expected,
            "{delta} {modifiers:?} on {platform:?}",
        );
    }
}

/// Each IME event as the crate's: a preedit with its cursor ordered (the
/// platform may report its ends either way), a commit, a disable as the
/// empty preedit that ends a composition, and an enable as nothing.
#[test]
fn ime_events_translate_and_a_disable_ends_the_composition() {
    use winit::event::Ime;

    let rows = [
        (
            Ime::Preedit("かな".into(), Some((6, 3))),
            Some("ImePreedit { text: \"かな\", cursor: Some(Span { start: 3, len: 3 }) }"),
        ),
        (
            Ime::Preedit("か".into(), None),
            Some("ImePreedit { text: \"か\", cursor: None }"),
        ),
        (Ime::Commit("仮名".into()), Some("ImeCommit(\"仮名\")")),
        (
            Ime::Disabled,
            Some("ImePreedit { text: \"\", cursor: None }"),
        ),
        (Ime::Enabled, None),
    ];
    for (ime, want) in rows {
        let event = WindowEvent::Ime(ime);
        let mut emitted = Vec::new();
        translate(&event, at(1.0), |event| emitted.push(format!("{event:?}")));
        assert_eq!(emitted, want.into_iter().collect::<Vec<_>>(), "{event:?}");
    }
}
