//! Keyboard event vocabulary, sized for what `TextEdit` asks of it: a
//! small [`Key`](crate::Key) enum covering navigation/editing keys plus
//! printable characters, a [`Modifiers`](crate::Modifiers) struct, and a
//! [`KeyPress`](crate::KeyPress) pairing them — all `Copy`, so
//! [`InputEvent`](crate::input::input_event::InputEvent) is too.
//!
//! Consumers: `TextEdit`, the [`Shortcut`](crate::Shortcut) matcher, and
//! global [`KeyboardWake`](crate::input::watch::KeyboardWake) watchers,
//! fed from the per-frame keypress queue drained during the frame.

pub(crate) mod key;
pub(crate) mod key_press;
pub(crate) mod modifiers;
