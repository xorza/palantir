//! Input trickling: within a frame, every level a reader consults stays constant, so each
//! event is read against the state it arrived in.

use crate::common::span::Span;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::pointer::PointerButton;
use std::collections::VecDeque;
use std::time::Duration;

/// Events held back for a later frame, and what the current frame already changed.
///
/// Widgets read keys, pointer edges and scroll deltas once a frame, at record, against the
/// focus owner, the modifiers and each button's capture as they stand after every admitted
/// event. An event that would change one of those levels under an earlier event of the frame
/// waits, and so does everything behind it. A modifier change waits rather than each pointer
/// event carrying its own set, because a frame's summed scroll delta has no single set.
#[derive(Debug, Default)]
pub(crate) struct InputQueue {
    /// Held events, oldest first; capacity retained.
    pending: VecDeque<HeldEvent>,
    /// Buttons whose capture began or ended this frame.
    buttons: [bool; PointerButton::COUNT],
    /// This frame's command key (typed nothing, not a bare modifier); repeats ride along.
    command_key: Option<Key>,
    /// The frame holds a key press, read by the widget focused at record: a focus move waits.
    key_pressed: bool,
    /// The frame holds a pointer edge or a scroll or pinch delta, read under the modifiers held
    /// at record: a modifier change waits.
    pointer_edge: bool,
    /// Text of every held IME event, end to end; each holds its span.
    text: String,
}

/// An event held for a later frame, with its arrival time.
#[derive(Clone, Copy, Debug)]
pub(super) struct HeldEvent {
    pub(super) event: InputEvent<'static>,
    pub(super) text: Span,
    pub(super) at: Duration,
}

/// A key press that names no key and types nothing: a modifier alone.
pub(super) fn is_bare_modifier(key: Key, text: KeyText) -> bool {
    key == Key::Other && text.is_empty()
}

impl InputQueue {
    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Whether `event` may apply this frame: it changes no level an earlier event of the frame was
    /// read against. Records nothing; the caller checks the queue is empty first, so held events
    /// keep their order, and notes what the event changed once it applies.
    pub(super) fn admits(&self, event: &InputEvent<'_>) -> bool {
        match *event {
            // A left press moves focus, so keys already read this frame would reach the new owner.
            InputEvent::PointerPressed(button) => {
                !self.buttons[button.idx()] && (button != PointerButton::Left || !self.key_pressed)
            }
            InputEvent::PointerReleased(button) => !self.buttons[button.idx()],
            InputEvent::KeyDown { key, repeat, .. } => match self.command_key {
                Some(held) => repeat && key == held,
                // Tab moves focus before any widget reads the keys typed ahead of it.
                None => key != Key::Tab || !self.key_pressed,
            },
            // Committed text is typed text.
            InputEvent::ImeCommit(_) => self.command_key.is_none(),
            InputEvent::ModifiersChanged(_) => !self.pointer_edge,
            // It ends every capture, so it waits for a frame that changed none.
            InputEvent::SurfaceFocusLost => !self.buttons.contains(&true),
            // A tap's press and release may share a frame, as they do in egui's: the key reads
            // pressed and released, and not held. Holding a release back a frame would cost every
            // fast typist a frame per key.
            InputEvent::KeyUp { .. }
            | InputEvent::PointerMoved(_)
            | InputEvent::PointerLeft
            | InputEvent::ScrollPixels(_)
            | InputEvent::ScrollLines(_)
            | InputEvent::Zoom(_)
            | InputEvent::ImePreedit(_) => true,
        }
    }

    /// `button`'s capture began or ended this frame; presses that hit nothing are not noted, so a
    /// click on inert surface asks for no frame.
    pub(super) const fn note_capture(&mut self, button: PointerButton) {
        self.buttons[button.idx()] = true;
        self.pointer_edge = true;
    }

    /// A scroll or pinch delta reached a reader.
    pub(super) const fn note_delta(&mut self) {
        self.pointer_edge = true;
    }

    /// A reader received the press of `key`. One that typed nothing is a command key: later
    /// presses wait a frame, except its repeats.
    pub(super) fn note_key(&mut self, key: Key, text: KeyText) {
        if is_bare_modifier(key, text) {
            return;
        }
        self.key_pressed = true;
        if text.is_empty() {
            self.command_key.get_or_insert(key);
        }
    }

    /// A reader received committed IME text.
    pub(super) const fn note_text(&mut self) {
        self.key_pressed = true;
    }

    pub(super) fn defer(&mut self, event: InputEvent<'_>, now: Duration) {
        if self.pending.is_empty() {
            self.text.clear();
        }
        let start = self.text.len();
        if let Some(text) = event.text() {
            self.text.push_str(text);
        }
        self.pending.push_back(HeldEvent {
            event: event.with_text(""),
            text: Span::from(start..self.text.len()),
            at: now,
        });
    }

    pub(super) fn text(&self, span: Span) -> &str {
        &self.text[span.range()]
    }

    pub(super) const fn next_frame(&mut self) {
        self.buttons = [false; PointerButton::COUNT];
        self.command_key = None;
        self.key_pressed = false;
        self.pointer_edge = false;
    }

    /// The oldest held event.
    pub(super) fn front(&self) -> Option<HeldEvent> {
        self.pending.front().copied()
    }

    pub(super) fn pop_front(&mut self) {
        self.pending.pop_front();
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::modifiers::Modifiers;
    use crate::input_state::input_queue::InputQueue;

    impl InputQueue {
        /// The modifier set of the last held `ModifiersChanged`, if any.
        pub(crate) fn last_held_modifiers(&self) -> Option<Modifiers> {
            self.pending.iter().rev().find_map(|held| match held.event {
                InputEvent::ModifiersChanged(mods) => Some(mods),
                _ => None,
            })
        }
    }
}

#[cfg(test)]
mod tests;
