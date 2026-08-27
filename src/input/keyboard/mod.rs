//! Keyboard event vocabulary. The shape was sized for `TextEdit`'s
//! step-1 needs: a small [`Key`](crate::Key) enum covering
//! navigation/editing keys plus printable characters, a
//! [`Modifiers`](crate::Modifiers) struct, and an inline
//! [`TextChunk`](crate::TextChunk) so
//! [`InputEvent`](crate::input::input_event::InputEvent) stays `Copy`.
//!
//! Consumers: `TextEdit`, the [`Shortcut`](crate::Shortcut) matcher, and
//! global [`KeyboardWake`](crate::input::watch::KeyboardWake) watchers,
//! fed from the per-frame keyboard-event queue drained during the frame.

pub(crate) mod key;
pub(crate) mod key_press;
pub(crate) mod keyboard_event;
pub(crate) mod modifiers;
pub(crate) mod text_chunk;
