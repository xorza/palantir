//! Input trickling: at most one state-changing event of each kind per
//! frame.
//!
//! A widget reads one collapsed state per frame, so two gestures between
//! frames would overwrite each other — a press and its release would
//! surface as a click with no `Down` frame, and an Escape and the key after
//! it would both land on the field Escape just blurred. Native toolkits
//! avoid this by dispatching one event at a time to the current focus
//! owner; Dear ImGui gets the same result in immediate mode by trickling
//! its event queue (`io.ConfigInputTrickleEventQueue`, on by default since
//! 1.87). [`InputQueue`] is that rule here: an event that would make a
//! frame ambiguous waits, with every event after it, for the next frame.

use crate::common::span::Span;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;
use std::collections::VecDeque;
use std::time::Duration;

/// Events held back for a later frame, and what the current frame has
/// already changed.
///
/// The frame boundary is the end of a frame: [`Self::next_frame`] forgets
/// what changed, and the caller replays what the new frame admits. An
/// event that arrives while nothing waits and changes nothing already
/// changed applies at once.
#[derive(Debug, Default)]
pub(crate) struct InputQueue {
    /// Held events, oldest first. Capacity is retained, so steady state
    /// allocates nothing.
    pending: VecDeque<HeldEvent>,
    /// Buttons pressed or released since the frame began.
    buttons: [bool; PointerButton::COUNT],
    /// This frame's command key — a key press that typed nothing and is
    /// not a bare modifier. A frame takes one; repeats of it ride along.
    command_key: Option<Key>,
    /// The text of every held IME event, end to end; each holds its span.
    /// Cleared when a new hold starts on an empty queue, so it never
    /// outgrows one burst of held events, and its capacity is retained.
    text: String,
}

/// An event held for a later frame, with the time it arrived.
///
/// An IME event's text is copied into the queue, because the host's
/// string does not outlive the call: `event` holds it empty, and `text`
/// is where it went.
#[derive(Clone, Copy, Debug)]
pub(super) struct HeldEvent {
    pub(super) event: InputEvent<'static>,
    pub(super) text: Span,
    pub(super) at: Duration,
}

impl InputQueue {
    /// Whether nothing waits.
    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Whether `event` may apply now: it changes nothing this frame
    /// already changed. Nothing may overtake a held event, so a
    /// non-empty queue admits nothing; the caller checks that first.
    ///
    /// Admitting records nothing. What counts as a change is decided by
    /// applying the event — [`Self::note_button`] and
    /// [`Self::note_command_key`] — because only then is it known whether
    /// the press latched a capture or the key reached a reader.
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
            | InputEvent::ImePreedit { .. }
            | InputEvent::ImeCommit(_) => true,
        }
    }

    /// `button`'s capture began or ended this frame. A press that hit
    /// nothing, or a release with no capture to end, changes no widget's
    /// state and is not noted: a frame can hold any number of those.
    pub(super) const fn note_button(&mut self, button: PointerButton) {
        self.buttons[button.idx()] = true;
    }

    /// A reader received `key`, a command key — a press that typed
    /// nothing and is not a bare modifier. Key presses after it wait for
    /// the next frame, except repeats of it.
    pub(super) fn note_command_key(&mut self, key: Key) {
        self.command_key.get_or_insert(key);
    }

    /// Hold `event` for a later frame.
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

    /// The text a held event's `span` names.
    pub(super) fn text(&self, span: Span) -> &str {
        &self.text[span.range()]
    }

    /// Start the next frame: forget what this one changed.
    pub(super) const fn next_frame(&mut self) {
        self.buttons = [false; PointerButton::COUNT];
        self.command_key = None;
    }

    /// The oldest held event, if the frame now admits it, taken off the
    /// queue. Applying it notes what it changes.
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
        /// The modifier set the last held `ModifiersChanged` carries, if
        /// one is held — what the modifiers will be once everything held
        /// lands.
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
