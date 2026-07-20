//! Translation from winit events into Aperture's native input vocabulary.

use glam::Vec2;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WinitKey, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::{Key, Modifiers, TextChunk};
use crate::input::pointer::PointerButton;
use crate::input::{self as native_input, InputEvent};

pub(crate) fn translate(event: &WindowEvent, scale_factor: f32, mut emit: impl FnMut(InputEvent)) {
    let scale = scale_factor.max(f32::EPSILON);
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            emit(InputEvent::PointerMoved(Vec2::new(
                position.x as f32 / scale,
                position.y as f32 / scale,
            )));
        }
        WindowEvent::CursorLeft { .. } => emit(InputEvent::PointerLeft),
        WindowEvent::MouseInput { state, button, .. } => {
            let button = match button {
                MouseButton::Left => PointerButton::Left,
                MouseButton::Right => PointerButton::Right,
                MouseButton::Middle => PointerButton::Middle,
                _ => return,
            };
            emit(match state {
                ElementState::Pressed => InputEvent::PointerPressed(button),
                ElementState::Released => InputEvent::PointerReleased(button),
            });
        }
        WindowEvent::PinchGesture { delta, .. } => {
            let factor = 1.0 + *delta as f32;
            if native_input::zoom_factor_is_valid(factor) {
                emit(InputEvent::Zoom(factor));
            }
        }
        WindowEvent::MouseWheel { delta, .. } => emit(match *delta {
            MouseScrollDelta::LineDelta(x, y) => InputEvent::ScrollLines(Vec2::new(-x, -y)),
            MouseScrollDelta::PixelDelta(position) => InputEvent::ScrollPixels(Vec2::new(
                -position.x as f32 / scale,
                -position.y as f32 / scale,
            )),
        }),
        WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
            emit(InputEvent::KeyDown {
                key: logical_key(&event.logical_key),
                repeat: event.repeat,
                physical: physical_key(&event.physical_key),
            });
        }
        WindowEvent::Ime(Ime::Commit(text)) => emit_text_chunks(text, &mut emit),
        WindowEvent::ModifiersChanged(modifiers) => {
            emit(InputEvent::ModifiersChanged(normalize_modifiers(
                &modifiers.state(),
            )));
        }
        _ => {}
    }
}

fn logical_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
        WinitKey::Named(NamedKey::ArrowUp) => Key::ArrowUp,
        WinitKey::Named(NamedKey::ArrowDown) => Key::ArrowDown,
        WinitKey::Named(NamedKey::Backspace) => Key::Backspace,
        WinitKey::Named(NamedKey::Delete) => Key::Delete,
        WinitKey::Named(NamedKey::Home) => Key::Home,
        WinitKey::Named(NamedKey::End) => Key::End,
        WinitKey::Named(NamedKey::PageUp) => Key::PageUp,
        WinitKey::Named(NamedKey::PageDown) => Key::PageDown,
        WinitKey::Named(NamedKey::Enter) => Key::Enter,
        WinitKey::Named(NamedKey::Tab) => Key::Tab,
        WinitKey::Named(NamedKey::Escape) => Key::Escape,
        WinitKey::Named(NamedKey::F1) => Key::F1,
        WinitKey::Named(NamedKey::F2) => Key::F2,
        WinitKey::Named(NamedKey::F3) => Key::F3,
        WinitKey::Named(NamedKey::F4) => Key::F4,
        WinitKey::Named(NamedKey::F5) => Key::F5,
        WinitKey::Named(NamedKey::F6) => Key::F6,
        WinitKey::Named(NamedKey::F7) => Key::F7,
        WinitKey::Named(NamedKey::F8) => Key::F8,
        WinitKey::Named(NamedKey::F9) => Key::F9,
        WinitKey::Named(NamedKey::F10) => Key::F10,
        WinitKey::Named(NamedKey::F11) => Key::F11,
        WinitKey::Named(NamedKey::F12) => Key::F12,
        WinitKey::Named(NamedKey::Space) => Key::Char(' '),
        WinitKey::Character(text) => text.chars().next().map(Key::Char).unwrap_or(Key::Other),
        _ => Key::Other,
    }
}

fn physical_key(physical: &PhysicalKey) -> Key {
    let PhysicalKey::Code(code) = physical else {
        return Key::Other;
    };
    match code {
        KeyCode::KeyA => Key::Char('a'),
        KeyCode::KeyB => Key::Char('b'),
        KeyCode::KeyC => Key::Char('c'),
        KeyCode::KeyD => Key::Char('d'),
        KeyCode::KeyE => Key::Char('e'),
        KeyCode::KeyF => Key::Char('f'),
        KeyCode::KeyG => Key::Char('g'),
        KeyCode::KeyH => Key::Char('h'),
        KeyCode::KeyI => Key::Char('i'),
        KeyCode::KeyJ => Key::Char('j'),
        KeyCode::KeyK => Key::Char('k'),
        KeyCode::KeyL => Key::Char('l'),
        KeyCode::KeyM => Key::Char('m'),
        KeyCode::KeyN => Key::Char('n'),
        KeyCode::KeyO => Key::Char('o'),
        KeyCode::KeyP => Key::Char('p'),
        KeyCode::KeyQ => Key::Char('q'),
        KeyCode::KeyR => Key::Char('r'),
        KeyCode::KeyS => Key::Char('s'),
        KeyCode::KeyT => Key::Char('t'),
        KeyCode::KeyU => Key::Char('u'),
        KeyCode::KeyV => Key::Char('v'),
        KeyCode::KeyW => Key::Char('w'),
        KeyCode::KeyX => Key::Char('x'),
        KeyCode::KeyY => Key::Char('y'),
        KeyCode::KeyZ => Key::Char('z'),
        KeyCode::Digit0 => Key::Char('0'),
        KeyCode::Digit1 => Key::Char('1'),
        KeyCode::Digit2 => Key::Char('2'),
        KeyCode::Digit3 => Key::Char('3'),
        KeyCode::Digit4 => Key::Char('4'),
        KeyCode::Digit5 => Key::Char('5'),
        KeyCode::Digit6 => Key::Char('6'),
        KeyCode::Digit7 => Key::Char('7'),
        KeyCode::Digit8 => Key::Char('8'),
        KeyCode::Digit9 => Key::Char('9'),
        KeyCode::Space => Key::Char(' '),
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::Escape => Key::Escape,
        KeyCode::F1 => Key::F1,
        KeyCode::F2 => Key::F2,
        KeyCode::F3 => Key::F3,
        KeyCode::F4 => Key::F4,
        KeyCode::F5 => Key::F5,
        KeyCode::F6 => Key::F6,
        KeyCode::F7 => Key::F7,
        KeyCode::F8 => Key::F8,
        KeyCode::F9 => Key::F9,
        KeyCode::F10 => Key::F10,
        KeyCode::F11 => Key::F11,
        KeyCode::F12 => Key::F12,
        _ => Key::Other,
    }
}

fn normalize_modifiers(modifiers: &ModifiersState) -> Modifiers {
    let mac = matches!(PLATFORM, Platform::Mac);
    Modifiers {
        shift: modifiers.shift_key(),
        ctrl: if mac {
            modifiers.super_key()
        } else {
            modifiers.control_key()
        },
        alt: modifiers.alt_key(),
        mac_ctrl: mac && modifiers.control_key(),
    }
}

fn emit_text_chunks(text: &str, emit: &mut impl FnMut(InputEvent)) {
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len().min(TextChunk::INLINE_CAP);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (head, tail) = rest.split_at(end);
        emit(InputEvent::Text(
            TextChunk::new(head).expect("chunk fits by construction"),
        ));
        rest = tail;
    }
}

#[cfg(test)]
mod tests;
