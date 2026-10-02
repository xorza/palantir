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

use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerButton;
use std::collections::VecDeque;
use std::time::Duration;
use strum::EnumCount as _;

/// Events held back for a later frame, and what the current frame has
/// already changed.
///
/// The frame boundary is the end of a frame: [`Self::next_frame`] forgets
/// what changed, and the caller replays what the new frame admits. An
/// event that arrives while nothing waits and changes nothing already
/// changed applies at once, as before.
#[derive(Debug, Default)]
pub(crate) struct InputQueue {
    /// Held events with the time each arrived, oldest first. Capacity is
    /// retained, so steady state allocates nothing.
    pending: VecDeque<(InputEvent, Duration)>,
    /// Buttons pressed or released since the frame began.
    buttons: [bool; PointerButton::COUNT],
    /// This frame's command key — a key press that typed nothing and is
    /// not a bare modifier. A frame takes one; repeats of it ride along.
    command_key: Option<Key>,
}

impl InputQueue {
    /// Whether nothing waits.
    pub(crate) fn is_empty(&self) -> bool {
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
    pub(crate) fn admits(&self, event: &InputEvent) -> bool {
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
            | InputEvent::SurfaceFocusLost => true,
        }
    }

    /// `button`'s capture began or ended this frame. A press that hit
    /// nothing, or a release with no capture to end, changes no widget's
    /// state and is not noted: a frame can hold any number of those.
    pub(crate) fn note_button(&mut self, button: PointerButton) {
        self.buttons[button.idx()] = true;
    }

    /// A reader received `key`, a command key — a press that typed
    /// nothing and is not a bare modifier. Key presses after it wait for
    /// the next frame, except repeats of it.
    pub(crate) fn note_command_key(&mut self, key: Key) {
        self.command_key.get_or_insert(key);
    }

    /// The modifier set the last held `ModifiersChanged` carries, if one
    /// is held — what the modifiers will be once everything held lands.
    pub(crate) fn last_held_modifiers(&self) -> Option<Modifiers> {
        self.pending
            .iter()
            .rev()
            .find_map(|(event, _)| match event {
                InputEvent::ModifiersChanged(mods) => Some(*mods),
                _ => None,
            })
    }

    /// Hold `event` for a later frame.
    pub(crate) fn defer(&mut self, event: InputEvent, now: Duration) {
        self.pending.push_back((event, now));
    }

    /// Start the next frame: forget what this one changed.
    pub(crate) fn next_frame(&mut self) {
        self.buttons = [false; PointerButton::COUNT];
        self.command_key = None;
    }

    /// The oldest held event, if the frame now admits it, taken off the
    /// queue. Applying it notes what it changes.
    pub(crate) fn pop_admitted(&mut self) -> Option<(InputEvent, Duration)> {
        let (event, _) = self.pending.front()?;
        if !self.admits(event) {
            return None;
        }
        self.pending.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::InputQueue;
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::key_text::KeyText;
    use crate::input::pointer::PointerButton;
    use std::time::Duration;

    fn key(key: Key, text: KeyText, repeat: bool) -> InputEvent {
        InputEvent::KeyDown {
            key,
            repeat,
            physical: Key::Other,
            text,
        }
    }

    fn typed(c: char) -> InputEvent {
        key(Key::Char(c), KeyText::from_char(c), false)
    }

    /// Admit `event` and note what it changes, as `InputState::apply`
    /// does when every press latches and every key reaches a reader.
    fn admit_and_apply(queue: &mut InputQueue, event: &InputEvent) -> bool {
        if !queue.admits(event) {
            return false;
        }
        match *event {
            InputEvent::PointerPressed(button) | InputEvent::PointerReleased(button) => {
                queue.note_button(button);
            }
            InputEvent::KeyDown { key, text, .. } if text.is_empty() && key != Key::Other => {
                queue.note_command_key(key);
            }
            _ => {}
        }
        true
    }

    /// The admission rules, one row per event sequence in a fresh frame.
    #[test]
    fn a_frame_admits_one_change_per_button_and_one_command_key() {
        let left = PointerButton::Left;
        let right = PointerButton::Right;
        let rows: &[(&str, &[InputEvent], &[bool])] = &[
            (
                "press then release of one button: the release waits",
                &[
                    InputEvent::PointerPressed(left),
                    InputEvent::PointerReleased(left),
                ],
                &[true, false],
            ),
            (
                "two buttons change in one frame",
                &[
                    InputEvent::PointerPressed(left),
                    InputEvent::PointerPressed(right),
                ],
                &[true, true],
            ),
            (
                "a typing run stays in one frame",
                &[typed('a'), typed('b'), typed('c')],
                &[true, true, true],
            ),
            (
                "text after a command key waits",
                &[key(Key::Escape, KeyText::EMPTY, false), typed('a')],
                &[true, false],
            ),
            (
                "a command key after text closes the run",
                &[
                    typed('a'),
                    key(Key::Backspace, KeyText::EMPTY, false),
                    typed('c'),
                ],
                &[true, true, false],
            ),
            (
                "repeats of the held command key ride along",
                &[
                    key(Key::Backspace, KeyText::EMPTY, false),
                    key(Key::Backspace, KeyText::EMPTY, true),
                    key(Key::Backspace, KeyText::EMPTY, true),
                ],
                &[true, true, true],
            ),
            (
                "a second command key waits",
                &[
                    key(Key::ArrowLeft, KeyText::EMPTY, false),
                    key(Key::ArrowRight, KeyText::EMPTY, false),
                ],
                &[true, false],
            ),
            (
                "a bare modifier does not end a typing run",
                &[key(Key::Other, KeyText::EMPTY, false), typed('a')],
                &[true, true],
            ),
        ];
        for (label, events, admitted) in rows {
            let mut queue = InputQueue::default();
            let got: Vec<bool> = events
                .iter()
                .map(|event| admit_and_apply(&mut queue, event))
                .collect();
            assert_eq!(&got, admitted, "{label}");
        }
    }

    /// Only what was noted holds the frame: a press that latched nothing
    /// leaves its release free to land in the same frame.
    #[test]
    fn an_unnoted_change_holds_nothing_back() {
        let left = PointerButton::Left;
        let queue = InputQueue::default();
        assert!(queue.admits(&InputEvent::PointerPressed(left)));
        assert!(queue.admits(&InputEvent::PointerReleased(left)));
    }

    /// A held event waits for `next_frame`, then pops in order, one
    /// frame's worth at a time.
    #[test]
    fn held_events_replay_in_order_one_frame_at_a_time() {
        let left = PointerButton::Left;
        let mut queue = InputQueue::default();
        assert!(admit_and_apply(
            &mut queue,
            &InputEvent::PointerPressed(left)
        ));
        for (event, ms) in [
            (InputEvent::PointerReleased(left), 1),
            (InputEvent::PointerPressed(left), 2),
            (InputEvent::PointerReleased(left), 3),
        ] {
            queue.defer(event, Duration::from_millis(ms));
        }
        assert!(queue.pop_admitted().is_none(), "the same frame admits none");

        let mut frames = Vec::new();
        while !queue.is_empty() {
            queue.next_frame();
            let mut frame = Vec::new();
            while let Some((event, at)) = queue.pop_admitted() {
                admit_and_apply(&mut queue, &event);
                frame.push(at.as_millis());
            }
            frames.push(frame);
        }
        assert_eq!(frames, [vec![1], vec![2], vec![3]], "arrival times kept");
    }
}
