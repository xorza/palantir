use glam::Vec2;
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, Ime, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::keyboard::{
    Key as WinitKey, KeyCode, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey,
};

use crate::common::platform::{PLATFORM, Platform};
use crate::host::winit::input::{logical_key, normalize_modifiers, physical_key, translate};
use crate::input::InputEvent;
use crate::input::keyboard::{Key, Modifiers};

fn wheel(delta: MouseScrollDelta) -> WindowEvent {
    WindowEvent::MouseWheel {
        device_id: DeviceId::dummy(),
        delta,
        phase: TouchPhase::Moved,
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
    let code = |code| physical_key(&PhysicalKey::Code(code));
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
        physical_key(&PhysicalKey::Unidentified(NativeKeyCode::Unidentified)),
        Key::Other
    );
}

#[test]
fn modifier_normalization_translates_each_bit() {
    let mac = matches!(PLATFORM, Platform::Mac);
    assert_eq!(
        normalize_modifiers(&ModifiersState::empty()),
        Modifiers::NONE
    );

    let modifiers = normalize_modifiers(&ModifiersState::SHIFT);
    assert!(modifiers.shift && !modifiers.ctrl && !modifiers.alt && !modifiers.mac_ctrl);
    let modifiers = normalize_modifiers(&ModifiersState::ALT);
    assert!(modifiers.alt && !modifiers.shift && !modifiers.ctrl && !modifiers.mac_ctrl);

    let primary = if mac {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    let modifiers = normalize_modifiers(&primary);
    assert!(modifiers.ctrl && !modifiers.shift && !modifiers.alt && !modifiers.mac_ctrl);

    let modifiers = normalize_modifiers(&ModifiersState::CONTROL);
    if mac {
        assert!(modifiers.mac_ctrl && !modifiers.ctrl);
    } else {
        assert!(modifiers.ctrl && !modifiers.mac_ctrl);
    }

    let modifiers = normalize_modifiers(&ModifiersState::SUPER);
    if mac {
        assert!(modifiers.ctrl && !modifiers.mac_ctrl);
    } else {
        assert!(!modifiers.ctrl && !modifiers.mac_ctrl);
    }

    let modifiers = normalize_modifiers(&(ModifiersState::SHIFT | primary));
    assert!(modifiers.shift && modifiers.ctrl && !modifiers.alt);
}

#[test]
fn ime_commits_roundtrip_through_inline_chunks() {
    for text in ["é", "0123456789abcdef", "日本語入力テスト文字列"] {
        let mut got = String::new();
        translate(
            &WindowEvent::Ime(Ime::Commit(text.into())),
            1.0,
            |event| match event {
                InputEvent::Text(chunk) => got.push_str(chunk.as_str()),
                other => panic!("unexpected {other:?}"),
            },
        );
        assert_eq!(got, text);
    }
}

#[test]
fn wheel_deltas_are_logical_and_point_in_scroll_direction() {
    let mut got = None;
    translate(
        &wheel(MouseScrollDelta::LineDelta(2.0, 1.0)),
        1.0,
        |event| got = Some(event),
    );
    assert!(matches!(
        got,
        Some(InputEvent::ScrollLines(delta)) if delta == Vec2::new(-2.0, -1.0)
    ));

    translate(
        &wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            60.0, -120.0,
        ))),
        2.0,
        |event| got = Some(event),
    );
    assert!(matches!(
        got,
        Some(InputEvent::ScrollPixels(delta)) if delta == Vec2::new(-30.0, 60.0)
    ));
}

#[test]
fn pinch_translation_rejects_invalid_factors() {
    let mut emitted = None;
    translate(&pinch(0.5), 1.0, |event| emitted = Some(event));
    assert!(matches!(emitted, Some(InputEvent::Zoom(1.5))));

    for delta in [-1.0, -2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut count = 0;
        translate(&pinch(delta), 1.0, |_| count += 1);
        assert_eq!(count, 0, "invalid pinch delta {delta:?} emitted an event");
    }
}
