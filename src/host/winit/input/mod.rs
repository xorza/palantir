//! Translation from winit events into Palantir's native input vocabulary.

use glam::Vec2;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WinitKey, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::common::platform::{PLATFORM, Platform};
use crate::display;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerButton;

pub(super) fn translate(event: &WindowEvent, scale_factor: f32, mut emit: impl FnMut(InputEvent)) {
    debug_assert!(
        display::scale_factor_is_valid(scale_factor),
        "the host screens the scale factor through display::sanitize_scale_factor; \
         got {scale_factor}",
    );
    let scale = scale_factor;
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            emit(InputEvent::PointerMoved(Vec2::new(
                position.x as f32 / scale,
                position.y as f32 / scale,
            )));
        }
        WindowEvent::CursorLeft { .. } => emit(InputEvent::PointerLeft),
        WindowEvent::MouseInput { state, button, .. } => {
            // The vocabulary stops at three on purpose:
            // [`PointerButton`] indexes a `ButtonState` on every
            // widget's `ResponseState` and a capture slot on
            // `InputState`, so a fourth is a per-widget-per-frame cost
            // paid by every app for a button almost none of them bind.
            // Named rather than swept into a wildcard, so adding one
            // here is a decision rather than a discovery.
            let button = match button {
                MouseButton::Left => PointerButton::Left,
                MouseButton::Right => PointerButton::Right,
                MouseButton::Middle => PointerButton::Middle,
                MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => return,
            };
            emit(match state {
                ElementState::Pressed => InputEvent::PointerPressed(button),
                ElementState::Released => InputEvent::PointerReleased(button),
            });
        }
        // A pinch delta is a displacement, so the factor is `1 + delta`.
        // Emitted unscreened like every other payload here: what a factor
        // has to satisfy is `InputEvent::is_valid`'s question.
        WindowEvent::PinchGesture { delta, .. } => {
            emit(InputEvent::Zoom(1.0 + *delta as f32));
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
        WindowEvent::ModifiersChanged(modifiers) => {
            emit(InputEvent::ModifiersChanged(normalize_modifiers(
                &modifiers.state(),
            )));
        }
        // Only the loss is forwarded: regaining focus tells the state
        // machine nothing it does not already learn from the next event.
        WindowEvent::Focused(false) => emit(InputEvent::SurfaceFocusLost),
        _ => {}
    }
}

/// The keys winit spells the same way in both of its vocabularies, and
/// the [`Key`] each denotes.
///
/// Written once and expanded against `NamedKey` (the logical side) and
/// `KeyCode` (the physical one). A macro rather than a lookup table
/// because the two winit enums share only these variant *names* — there
/// is nothing to index, just a shape to repeat.
///
/// The two must agree: `Shortcut::matches`'s non-Latin fallback
/// (`src/input/shortcut/mod.rs`) consults `physical` alone, so a key present
/// on one side and missing from the other stops matching under a
/// non-Latin layout and nothing says so.
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
            // The one whose `Key` is not the same name.
            $winit::Space => Some(Key::Char(' ')),
            _ => None,
        }
    };
}

/// The [`Key`] a winit logical key denotes, or [`Key::Other`] where the
/// vocabulary has no name for it.
///
/// A `Character` payload is a string because a dead-key sequence can
/// resolve to several chars, and only the first is taken: [`Key`] names a
/// *key*, and a multi-char resolution is text rather than a key. The rest
/// is dropped, because treating the run as a chord would bind shortcuts
/// to whichever char happened to come first. A host that wants the whole
/// resolution wants IME, which this one does not enable.
fn logical_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Named(named) => shared_key!(NamedKey, named).unwrap_or(Key::Other),
        WinitKey::Character(text) => text.chars().next().map(Key::Char).unwrap_or(Key::Other),
        _ => Key::Other,
    }
}

/// The Latin letter and digit positions, which exist only on the
/// physical side — the logical side reports whatever the layout puts
/// there, as a `Character`.
fn latin_position(code: &KeyCode) -> Option<Key> {
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

fn physical_key(physical: &PhysicalKey) -> Key {
    let PhysicalKey::Code(code) = physical else {
        return Key::Other;
    };
    // `or`, not `or_else`: the macro expands to a match, not a call, so
    // there is nothing to defer.
    latin_position(code)
        .or(shared_key!(KeyCode, code))
        .unwrap_or(Key::Other)
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

#[cfg(test)]
mod tests;
