use glam::Vec2;
use winit::keyboard::{
    Key as WinitKey, KeyCode, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey,
};

use crate::common::platform::Platform;
use crate::host::winit::input::wheel_steps::WheelSteps;
use crate::host::winit::input::{
    KeyDownFacts, Press, key_down, logical_key, normalize_modifiers, physical_key, shift_wheel,
    wheel_lines,
};
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_text::KeyText;
use crate::primitives::input::keyboard::modifiers::Modifiers;

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

/// Each winit modifier per platform: macOS Cmd is the command bit and Control is `MAC_CTRL`; elsewhere Control is.
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
            none | Modifiers::MAC_CTRL,
        ),
        (ModifiersState::CONTROL, Platform::Windows, Modifiers::CTRL),
        (
            ModifiersState::SUPER,
            Platform::Linux,
            none | Modifiers::META,
        ),
        (
            ModifiersState::SHIFT | ModifiersState::CONTROL,
            Platform::Linux,
            Modifiers::CTRL | Modifiers::SHIFT,
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
        Some(Press {
            key_down: InputEvent::KeyDown {
                key: Key::Enter,
                ..
            },
            overflow: None,
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
    assert!(typed(facts, ModifiersState::SUPER).is_empty());
    assert_eq!(typed(facts, ModifiersState::empty()).as_str(), "l");
    assert_eq!(typed(facts, ModifiersState::SHIFT).as_str(), "l");
}

/// The text the `KeyDown` of a press of `facts` under `modifiers` carries.
fn typed(facts: KeyDownFacts<'_>, modifiers: ModifiersState) -> KeyText {
    match key_down(facts, modifiers) {
        Some(Press {
            key_down: InputEvent::KeyDown { text, .. },
            ..
        }) => text,
        other => panic!("a press translates to a KeyDown, got {other:?}"),
    }
}

/// The text a press of `facts` under `modifiers` types past its `KeyDown`.
fn overflow(facts: KeyDownFacts<'_>, modifiers: ModifiersState) -> Option<&str> {
    key_down(facts, modifiers)
        .expect("a press is input")
        .overflow
}

/// Text past what a `KeyDown` carries overflows whole characters into committed text, so none is
/// lost. 20 bytes, a family emoji of three people joined by two zero-width joiners (18) and
/// "ab": the press carries the first person and joiner twice, 14 bytes, and the rest, the last
/// person and "ab", follows. Exactly 14 bytes overflow nothing, and Super clears the overflow
/// with the text.
#[test]
fn text_past_a_key_overflows_into_committed_text() {
    let logical = WinitKey::Character("a".into());
    let physical = PhysicalKey::Code(KeyCode::KeyA);
    let facts = |text| KeyDownFacts {
        logical: &logical,
        physical: &physical,
        text: Some(text),
        repeat: false,
        is_synthetic: false,
    };
    let family = "\u{1F469}\u{200D}\u{1F469}\u{200D}\u{1F466}ab";
    assert_eq!(family.len(), 20);
    let none = ModifiersState::empty();
    let head = typed(facts(family), none);
    let tail = overflow(facts(family), none);
    assert_eq!(head.as_str(), "\u{1F469}\u{200D}\u{1F469}\u{200D}");
    assert_eq!(tail, Some("\u{1F466}ab"));
    assert_eq!(format!("{}{}", head.as_str(), tail.unwrap()), family);

    let fourteen = "abcdefghijklmn";
    assert_eq!(typed(facts(fourteen), none).as_str(), fourteen);
    assert_eq!(overflow(facts(fourteen), none), None);
    assert!(typed(facts(family), ModifiersState::SUPER).is_empty());
    assert_eq!(overflow(facts(family), ModifiersState::SUPER), None);
}

/// Each lane of a wheel event: `notches` as the wheel turned, the others by the setting, and Shift
/// turning all of them alike. A setting of no lines still turns `notches`, as `lines` tests nothing.
#[test]
fn a_wheel_event_carries_each_lane() {
    let three = WheelSteps {
        vertical_lines: Some(3.0),
        horizontal_lines: 3.0,
    };
    let none = WheelSteps {
        vertical_lines: Some(0.0),
        horizontal_lines: 0.0,
    };
    let page = WheelSteps {
        vertical_lines: None,
        horizontal_lines: 3.0,
    };
    let empty = ModifiersState::empty();
    let shift = ModifiersState::SHIFT;
    let down = Vec2::new(0.0, 2.0);
    let rows = [
        (
            WheelSteps::default(),
            empty,
            Platform::Linux,
            [0.0, 2.0],
            [0.0, 2.0],
            [0.0, 0.0],
        ),
        // 2 notches × 3 lines.
        (
            three,
            empty,
            Platform::Windows,
            [0.0, 2.0],
            [0.0, 6.0],
            [0.0, 0.0],
        ),
        (
            three,
            shift,
            Platform::Windows,
            [2.0, 0.0],
            [6.0, 0.0],
            [0.0, 0.0],
        ),
        (
            none,
            shift,
            Platform::Windows,
            [2.0, 0.0],
            [0.0, 0.0],
            [0.0, 0.0],
        ),
        (
            page,
            empty,
            Platform::Windows,
            [0.0, 2.0],
            [0.0, 0.0],
            [0.0, 2.0],
        ),
        (
            page,
            shift,
            Platform::Windows,
            [2.0, 0.0],
            [0.0, 0.0],
            [2.0, 0.0],
        ),
        (
            three,
            shift,
            Platform::Mac,
            [0.0, 2.0],
            [0.0, 6.0],
            [0.0, 0.0],
        ),
    ];
    for (steps, modifiers, platform, want_notches, want_lines, want_pages) in rows {
        let InputEvent::ScrollLines {
            notches,
            lines,
            pages,
        } = wheel_lines(down, steps, modifiers, platform)
        else {
            panic!("a wheel turn is a line event");
        };
        let case = format!("{steps:?} {modifiers:?} on {platform:?}");
        assert_eq!(notches, Vec2::from(want_notches), "{case}");
        assert_eq!(lines, Vec2::from(want_lines), "{case}");
        assert_eq!(pages, Vec2::from(want_pages), "{case}");
    }
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
