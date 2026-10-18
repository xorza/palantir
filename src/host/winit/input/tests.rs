use glam::Vec2;
use winit::keyboard::{
    Key as WinitKey, KeyCode, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey,
};

use crate::common::platform::Platform;
use crate::host::winit::input::{
    KeyDownFacts, key_down, logical_key, normalize_modifiers, physical_key, shift_wheel,
};
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;

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

/// Each winit modifier per platform: macOS Cmd is the command bit and Control is `mac_ctrl`; elsewhere Control is.
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

/// The logical and physical key tables denote the same [`Key`]; `Shortcut::matches`' non-Latin fallback uses `physical`
/// alone, so a one-sided arm would silently stop matching under a non-Latin layout.
#[test]
fn shared_keys_denote_the_same_key_on_both_sides() {
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

/// A synthetic press (winit's replay of a key held at window focus) is not input; a real press with the same key is.
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

/// Super has no `Modifiers` bit off macOS, so text typed under it is cleared; without Super it is kept.
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

/// Shift+wheel scrolls sideways on Windows and Linux (only a purely vertical delta moves); macOS sends it itself.
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
