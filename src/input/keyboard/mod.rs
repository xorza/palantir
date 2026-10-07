//! Keyboard event vocabulary sized for `TextEdit`: a small [`Key`](crate::Key) enum, [`Modifiers`](crate::Modifiers), the [`KeyText`](crate::KeyText) a press produced, and a [`KeyPress`](crate::KeyPress) pairing them, all `Copy` so `InputEvent` is too.
//!
//! Consumers: `TextEdit`, the [`Shortcut`](crate::Shortcut) matcher, and [`KeyboardWake`](crate::input::watch::KeyboardWake) watchers, fed from the per-frame keypress queue.

pub(crate) mod key;
pub(crate) mod key_press;
pub(crate) mod key_text;
pub(crate) mod modifiers;
