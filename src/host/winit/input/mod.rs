//! Translation from winit events into Palantir's native input vocabulary.

use glam::Vec2;
use winit::event::Ime;
use winit::keyboard::{Key as WinitKey, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::common::platform::Platform;
use crate::common::span::Span;
use crate::input::ime_preedit::ImePreedit;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;

pub(super) mod translator;

/// Keys winit spells the same in `NamedKey` (logical) and `KeyCode`
/// (physical), and the [`Key`] each denotes. A macro because the enums share
/// only variant names.
///
/// The sides must agree: `Shortcut::matches`'s non-Latin fallback
/// (`src/input/shortcut/mod.rs`) consults `physical` alone.
macro_rules! shared_key {
    ($winit:ident, $value:expr) => {
        match $value {
            $winit::ArrowLeft => Some(Key::ArrowLeft),
            $winit::ArrowRight => Some(Key::ArrowRight),
            $winit::ArrowUp => Some(Key::ArrowUp),
            $winit::ArrowDown => Some(Key::ArrowDown),
            $winit::Backspace => Some(Key::Backspace),
            $winit::Delete => Some(Key::Delete),
            $winit::Home => Some(Key::Home),
            $winit::End => Some(Key::End),
            $winit::PageUp => Some(Key::PageUp),
            $winit::PageDown => Some(Key::PageDown),
            $winit::Enter => Some(Key::Enter),
            $winit::Tab => Some(Key::Tab),
            $winit::Escape => Some(Key::Escape),
            $winit::F1 => Some(Key::F1),
            $winit::F2 => Some(Key::F2),
            $winit::F3 => Some(Key::F3),
            $winit::F4 => Some(Key::F4),
            $winit::F5 => Some(Key::F5),
            $winit::F6 => Some(Key::F6),
            $winit::F7 => Some(Key::F7),
            $winit::F8 => Some(Key::F8),
            $winit::F9 => Some(Key::F9),
            $winit::F10 => Some(Key::F10),
            $winit::F11 => Some(Key::F11),
            $winit::F12 => Some(Key::F12),
            $winit::Space => Some(Key::Char(' ')),
            _ => None,
        }
    };
}

/// The [`Key`] a winit logical key denotes, or [`Key::Other`]. For a
/// `Character`, only the first char of a dead-key sequence is taken, since
/// [`Key`] names a key; the resolved text travels in the event's `text`.
fn logical_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Named(named) => shared_key!(NamedKey, named).unwrap_or(Key::Other),
        WinitKey::Character(text) => text.chars().next().map_or(Key::Other, Key::Char),
        _ => Key::Other,
    }
}

/// Latin letter and digit positions, which exist only on the physical side.
const fn latin_pos(code: KeyCode) -> Option<Key> {
    let c = match code {
        KeyCode::KeyA => 'a',
        KeyCode::KeyB => 'b',
        KeyCode::KeyC => 'c',
        KeyCode::KeyD => 'd',
        KeyCode::KeyE => 'e',
        KeyCode::KeyF => 'f',
        KeyCode::KeyG => 'g',
        KeyCode::KeyH => 'h',
        KeyCode::KeyI => 'i',
        KeyCode::KeyJ => 'j',
        KeyCode::KeyK => 'k',
        KeyCode::KeyL => 'l',
        KeyCode::KeyM => 'm',
        KeyCode::KeyN => 'n',
        KeyCode::KeyO => 'o',
        KeyCode::KeyP => 'p',
        KeyCode::KeyQ => 'q',
        KeyCode::KeyR => 'r',
        KeyCode::KeyS => 's',
        KeyCode::KeyT => 't',
        KeyCode::KeyU => 'u',
        KeyCode::KeyV => 'v',
        KeyCode::KeyW => 'w',
        KeyCode::KeyX => 'x',
        KeyCode::KeyY => 'y',
        KeyCode::KeyZ => 'z',
        KeyCode::Digit0 => '0',
        KeyCode::Digit1 => '1',
        KeyCode::Digit2 => '2',
        KeyCode::Digit3 => '3',
        KeyCode::Digit4 => '4',
        KeyCode::Digit5 => '5',
        KeyCode::Digit6 => '6',
        KeyCode::Digit7 => '7',
        KeyCode::Digit8 => '8',
        KeyCode::Digit9 => '9',
        _ => return None,
    };
    Some(Key::Char(c))
}

fn physical_key(physical: PhysicalKey) -> Key {
    let PhysicalKey::Code(code) = physical else {
        return Key::Other;
    };
    // `or`, not `or_else`: the macro expands to a match, so nothing to defer.
    latin_pos(code)
        .or(shared_key!(KeyCode, code))
        .unwrap_or(Key::Other)
}

/// A vertical wheel with Shift held scrolls horizontally on Windows and
/// Linux; macOS sends the horizontal delta itself.
fn shift_wheel(delta: Vec2, modifiers: ModifiersState, platform: Platform) -> Vec2 {
    let swaps = platform != Platform::Mac && modifiers.shift_key() && delta.x == 0.0;
    if swaps {
        Vec2::new(delta.y, 0.0)
    } else {
        delta
    }
}

/// The fields of a winit key press that translation reads; a `KeyEvent`
/// cannot be built outside winit.
#[derive(Clone, Copy, Debug)]
struct KeyDownFacts<'a> {
    logical: &'a WinitKey,
    physical: &'a PhysicalKey,
    text: Option<&'a str>,
    repeat: bool,
    is_synthetic: bool,
}

/// An IME event as the crate's. A disabled input method ends composing (the
/// empty preedit); enabling says nothing a widget acts on. A cursor is two byte
/// offsets, in either order.
fn ime_event(ime: &Ime) -> Option<InputEvent<'_>> {
    match ime {
        Ime::Preedit(text, cursor) => Some(InputEvent::ImePreedit(ImePreedit {
            text,
            cursor: cursor.map(|(a, b)| Span::from(a.min(b)..a.max(b))),
        })),
        Ime::Commit(text) => Some(InputEvent::ImeCommit(text)),
        Ime::Disabled => Some(InputEvent::ImePreedit(ImePreedit {
            text: "",
            cursor: None,
        })),
        Ime::Enabled => None,
    }
}

/// The `KeyDown` a press becomes, or `None` for one that is not input.
///
/// - A synthetic press is dropped: X11 and Windows send one per held key on
///   focus gain, which could submit a form nobody pressed a key at.
/// - Text typed with Super held is cleared: Super is a command modifier
///   everywhere (Cmd on macOS reaches `Modifiers::ctrl`), and `Modifiers` has
///   no Super bit on Windows and Linux.
fn key_down(facts: KeyDownFacts<'_>, modifiers: ModifiersState) -> Option<InputEvent<'static>> {
    if facts.is_synthetic {
        return None;
    }
    // The platform's resolution of layout, dead keys and modifiers; `logical_key`
    // holds one character where a dead-key fallback produces two.
    let text = match facts.text {
        Some(text) if !modifiers.super_key() => KeyText::new(text),
        _ => KeyText::EMPTY,
    };
    Some(InputEvent::KeyDown {
        key: logical_key(facts.logical),
        repeat: facts.repeat,
        physical: physical_key(*facts.physical),
        text,
    })
}

fn normalize_modifiers(modifiers: ModifiersState, platform: Platform) -> Modifiers {
    let mac = matches!(platform, Platform::Mac);
    Modifiers {
        shift: modifiers.shift_key(),
        ctrl: if mac {
            modifiers.super_key()
        } else {
            modifiers.control_key()
        },
        alt: modifiers.alt_key(),
        mac_ctrl: mac && modifiers.control_key(),
        meta: !mac && modifiers.super_key(),
    }
}

#[cfg(test)]
mod tests;
