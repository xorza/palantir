//! Input trickling: within a frame, every level a reader consults stays constant, so each
//! event is read against the state it arrived in.

use crate::common::span::Span;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_text::KeyText;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::PointerButton;
use std::collections::VecDeque;
use std::time::Duration;

/// The most events the queue holds. With moves and scroll deltas merged, at most one of each sits
/// between two discrete events a person made, so the queue fills only while frames stall: 1024
/// is about five seconds of typing at 100 keys a second, a move between each. A full queue
/// flushes ([`InputQueue::is_full`]).
pub(super) const MAX_HELD: usize = 1024;

/// Events held back for a later frame, and what the current frame already changed.
///
/// Widgets read keys, pointer edges and scroll deltas once a frame, at record, against the
/// focus owner, the modifiers and each button's capture as they stand after every admitted
/// event. An event that would change one of those levels under an earlier event of the frame
/// waits, and so does everything behind it. A modifier change waits rather than each pointer
/// event carrying its own set, because a frame's summed scroll delta has no single set.
#[derive(Debug, Default)]
pub(crate) struct InputQueue {
    /// Held events, oldest first, at most [`MAX_HELD`]; capacity retained.
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
    /// Text of every held IME event, end to end; each holds its span. The text before the oldest
    /// held span was read and is reclaimed once it is half the buffer, so the buffer holds at
    /// most about twice the held text, however long a stream keeps the queue from emptying.
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

    /// Whether the queue holds [`MAX_HELD`] events, so the next one flushes it: every held event
    /// applies at once, as Win32 refuses a post past its queue's limit
    /// (<https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-postmessagew>) but here
    /// with no event lost. Only the frame that flushes reads a key against a later focus or
    /// modifier set.
    pub(super) fn is_full(&self) -> bool {
        debug_assert!(
            self.pending.len() <= MAX_HELD,
            "the queue flushes at its cap"
        );
        self.pending.len() == MAX_HELD
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
            | InputEvent::Scroll(_)
            | InputEvent::Pinch(_)
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

    /// Hold `event` behind the held ones. A move right behind a held move replaces it, and a
    /// scroll delta right behind one of its kind adds to it, as Qt's
    /// `AA_CompressHighFrequencyEvents` merges them
    /// (<https://doc.qt.io/qt-6/qt.html#ApplicationAttribute-enum>): adjacent held events apply in
    /// one frame at one pointer position, so every level and the frame's summed deltas read the
    /// same. A reader of [`Ui::pointer_events`](crate::Ui::pointer_events) sees one move or scroll
    /// where several arrived, as a browser coalesces them; only events held behind a discrete one
    /// merge, which a stroke drawn while frames run rarely is.
    pub(super) fn defer(&mut self, event: InputEvent<'_>, now: Duration) {
        debug_assert!(!self.is_full(), "a full queue flushes before it holds more");
        if let Some(last) = self.pending.back_mut() {
            let merged = match (last.event, &event) {
                (InputEvent::PointerMoved(_), &InputEvent::PointerMoved(pos)) => {
                    Some(InputEvent::PointerMoved(pos))
                }
                (InputEvent::Scroll(held), &InputEvent::Scroll(delta)) => {
                    Some(InputEvent::Scroll(held + delta))
                }
                _ => None,
            };
            if let Some(merged) = merged {
                last.event = merged;
                last.at = now;
                return;
            }
        }
        self.reclaim_read_text();
        let start = self.text.len();
        if let Some(text) = event.text() {
            self.text.push_str(text);
        }
        self.pending.push_back(HeldEvent {
            event: event.with_text(""),
            text: Span::try_from(start..self.text.len()).unwrap(),
            at: now,
        });
    }

    /// Drop the text before the oldest held span once it is half the buffer, every span rebased.
    /// Here rather than at a pop, as the popped event's text is read right after it.
    fn reclaim_read_text(&mut self) {
        let read = self
            .pending
            .front()
            .map_or(self.text.len(), |held| held.text.start as usize);
        if read == 0 || read * 2 < self.text.len() {
            return;
        }
        self.text.drain(..read);
        let read = u32::try_from(read).expect("a span's start fits its u32");
        for held in &mut self.pending {
            held.text.start -= read;
        }
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

impl InputQueue {
    /// The modifier set of the last held `ModifiersChanged`, if any.
    pub(crate) fn last_held_modifiers(&self) -> Option<Modifiers> {
        self.pending.iter().rev().find_map(|held| match held.event {
            InputEvent::ModifiersChanged(mods) => Some(mods),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests;
