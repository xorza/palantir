//! Input trickling: at most one state-changing event of each kind per frame.

use crate::common::span::Span;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;
use std::collections::VecDeque;
use std::time::Duration;

/// Events held back for a later frame, and what the current frame already changed.
#[derive(Debug, Default)]
pub(crate) struct InputQueue {
    /// Held events, oldest first; capacity retained.
    pending: VecDeque<HeldEvent>,
    buttons: [bool; PointerButton::COUNT],
    /// This frame's command key (typed nothing, not a bare modifier); repeats ride along.
    command_key: Option<Key>,
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

impl InputQueue {
    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Whether `event` may apply now: it changes nothing this frame already changed. Records nothing; the caller checks the queue is empty first.
    pub(super) fn admits(&self, event: &InputEvent<'_>) -> bool {
        match *event {
            InputEvent::PointerPressed(button) | InputEvent::PointerReleased(button) => {
                !self.buttons[button.idx()]
            }
            InputEvent::KeyDown { key, repeat, .. } => match self.command_key {
                Some(held) => repeat && key == held,
                None => true,
            },
            InputEvent::PointerMoved(_)
            | InputEvent::PointerLeft
            | InputEvent::ScrollPixels(_)
            | InputEvent::ScrollLines(_)
            | InputEvent::Zoom(_)
            | InputEvent::ModifiersChanged(_)
            | InputEvent::SurfaceFocusLost
            | InputEvent::ImePreedit(_)
            | InputEvent::ImeCommit(_) => true,
        }
    }

    /// `button`'s capture began or ended this frame; presses that hit nothing are not noted.
    pub(super) const fn note_button(&mut self, button: PointerButton) {
        self.buttons[button.idx()] = true;
    }

    /// A reader received command key `key`; later key presses wait a frame, except repeats of it.
    pub(super) fn note_command_key(&mut self, key: Key) {
        self.command_key.get_or_insert(key);
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
    }

    /// The oldest held event if the frame now admits it; applying it notes what it changes.
    pub(super) fn pop_admitted(&mut self) -> Option<HeldEvent> {
        let held = self.pending.front()?;
        if !self.admits(&held.event) {
            return None;
        }
        self.pending.pop_front()
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::input::input_event::InputEvent;
    use crate::input::input_queue::InputQueue;
    use crate::input::keyboard::modifiers::Modifiers;

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
