use crate::input::ime_preedit::ImePreedit;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerButton;
use crate::input_state::input_queue::InputQueue;
use glam::Vec2;
use std::time::Duration;

fn key(key: Key, text: KeyText) -> InputEvent<'static> {
    InputEvent::KeyDown {
        key,
        repeat: false,
        physical: Key::Other,
        text,
    }
}

/// An auto-repeat of the text-less `key`.
fn key_repeat(key: Key) -> InputEvent<'static> {
    InputEvent::KeyDown {
        key,
        repeat: true,
        physical: Key::Other,
        text: KeyText::EMPTY,
    }
}

fn typed(c: char) -> InputEvent<'static> {
    key(Key::Char(c), KeyText::from_char(c))
}

/// Admit `event` and, as a reader receiving it, note what it changed: a press or release here
/// always lands on a widget, and every key reaches one.
fn admit(queue: &mut InputQueue, event: &InputEvent<'_>) -> bool {
    if !queue.admits(event) {
        return false;
    }
    match *event {
        InputEvent::PointerPressed(button) | InputEvent::PointerReleased(button) => {
            queue.note_capture(button);
        }
        InputEvent::ScrollPixels(_) | InputEvent::ScrollLines(_) | InputEvent::Zoom(_) => {
            queue.note_delta();
        }
        InputEvent::KeyDown { key, text, .. } => queue.note_key(key, text),
        InputEvent::ImeCommit(_) => queue.note_text(),
        InputEvent::PointerMoved(_)
        | InputEvent::PointerLeft
        | InputEvent::KeyUp { .. }
        | InputEvent::ModifiersChanged(_)
        | InputEvent::SurfaceFocusLost
        | InputEvent::ImePreedit(_) => {}
    }
    true
}

/// Within one frame, every level a reader consults stays constant: each row feeds one frame's
/// events in order and lists which the queue admits.
#[test]
fn a_frame_admits_only_events_that_keep_its_levels() {
    let left = PointerButton::Left;
    let right = PointerButton::Right;
    let shift_up = InputEvent::ModifiersChanged(Modifiers::NONE);
    let rows: &[(&str, &[InputEvent<'_>], &[bool])] = &[
        (
            "press then release of one button: the release waits",
            &[
                InputEvent::PointerPressed(left),
                InputEvent::PointerReleased(left),
            ],
            &[true, false],
        ),
        (
            "a key's press, release and press again share a frame: a tap costs no frame",
            &[
                typed('a'),
                InputEvent::KeyUp {
                    key: Key::Char('a'),
                },
                typed('a'),
            ],
            &[true, true, true],
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
            &[key(Key::Escape, KeyText::EMPTY), typed('a')],
            &[true, false],
        ),
        (
            "a command key after text closes the run",
            &[typed('a'), key(Key::Backspace, KeyText::EMPTY), typed('c')],
            &[true, true, false],
        ),
        (
            "repeats of the held command key ride along",
            &[
                key(Key::Backspace, KeyText::EMPTY),
                key_repeat(Key::Backspace),
                key_repeat(Key::Backspace),
            ],
            &[true, true, true],
        ),
        (
            "a second command key waits",
            &[
                key(Key::ArrowLeft, KeyText::EMPTY),
                key(Key::ArrowRight, KeyText::EMPTY),
            ],
            &[true, false],
        ),
        (
            "a bare modifier does not end a typing run",
            &[key(Key::Other, KeyText::EMPTY), typed('a')],
            &[true, true],
        ),
        (
            "Tab after typed keys waits, so they reach the field they were typed in",
            &[typed('a'), key(Key::Tab, KeyText::EMPTY)],
            &[true, false],
        ),
        (
            "Tab alone moves focus at once",
            &[key(Key::Tab, KeyText::EMPTY)],
            &[true],
        ),
        (
            "a left press after a typed key waits; another button does not move focus",
            &[
                typed('x'),
                InputEvent::PointerPressed(right),
                InputEvent::PointerPressed(left),
            ],
            &[true, true, false],
        ),
        (
            "a key after a left press reaches the new focus",
            &[InputEvent::PointerPressed(left), typed('x')],
            &[true, true],
        ),
        (
            "a modifier change after a press waits, so the press reads the set it came under",
            &[InputEvent::PointerPressed(left), shift_up],
            &[true, false],
        ),
        (
            "a modifier change after a scroll or pinch delta waits",
            &[InputEvent::ScrollLines(Vec2::Y), shift_up],
            &[true, false],
        ),
        (
            "a modifier change before a press applies with it",
            &[shift_up, InputEvent::PointerPressed(left), typed('x')],
            &[true, true, true],
        ),
        (
            "a pointer move is no edge: a modifier change after it applies",
            &[InputEvent::PointerMoved(Vec2::ONE), shift_up],
            &[true, true],
        ),
        (
            "an IME commit after a command key waits, as typed text does",
            &[key(Key::Enter, KeyText::EMPTY), InputEvent::ImeCommit("x")],
            &[true, false],
        ),
        (
            "an IME commit is a key press: Tab after it waits",
            &[InputEvent::ImeCommit("x"), key(Key::Tab, KeyText::EMPTY)],
            &[true, false],
        ),
        (
            "focus loss ends every capture, so it waits for a frame that changed none",
            &[
                InputEvent::PointerPressed(left),
                InputEvent::SurfaceFocusLost,
            ],
            &[true, false],
        ),
    ];
    for (label, events, admitted) in rows {
        let mut queue = InputQueue::default();
        let got: Vec<bool> = events
            .iter()
            .map(|event| admit(&mut queue, event))
            .collect();
        assert_eq!(&got, admitted, "{label}");
    }
}

/// A held event waits for `next_frame`, then pops in order, a frame's worth at a time.
#[test]
fn held_events_replay_in_order_one_frame_at_a_time() {
    let left = PointerButton::Left;
    let mut queue = InputQueue::default();
    assert!(admit(&mut queue, &InputEvent::PointerPressed(left)));
    for (event, ms) in [
        (InputEvent::PointerReleased(left), 1),
        (InputEvent::PointerPressed(left), 2),
        (InputEvent::PointerReleased(left), 3),
    ] {
        queue.defer(event, Duration::from_millis(ms));
    }
    let held = queue.front().expect("held");
    assert!(
        !admit(&mut queue, &held.event),
        "the same frame admits none"
    );

    let mut frames = Vec::new();
    while !queue.is_empty() {
        queue.next_frame();
        let mut frame = Vec::new();
        while let Some(held) = queue.front()
            && admit(&mut queue, &held.event)
        {
            queue.pop_front();
            frame.push(held.at.as_millis());
        }
        frames.push(frame);
    }
    assert_eq!(frames, [vec![1], vec![2], vec![3]], "arrival times kept");
}

/// A held IME event outlives the host's string: the queue copies its text and returns each event its own, in order.
#[test]
fn a_held_ime_event_keeps_its_own_text() {
    use crate::common::span::Span;

    let mut queue = InputQueue::default();
    for text in ["かな", "x"] {
        let owned = String::from(text);
        queue.defer(InputEvent::ImeCommit(&owned), Duration::ZERO);
    }
    let owned = String::from("abc");
    queue.defer(
        InputEvent::ImePreedit(ImePreedit {
            text: &owned,
            cursor: Some(Span::new(1, 1)),
        }),
        Duration::ZERO,
    );
    drop(owned);
    let mut got = Vec::new();
    while let Some(held) = queue.front()
        && admit(&mut queue, &held.event)
    {
        queue.pop_front();
        let event = held.event.with_text(queue.text(held.text));
        got.push(format!("{event:?}"));
    }
    assert_eq!(
        got,
        [
            "ImeCommit(\"かな\")",
            "ImeCommit(\"x\")",
            "ImePreedit(ImePreedit { text: \"abc\", cursor: Some(Span { start: 1, len: 1 }) })",
        ],
    );
    queue.defer(InputEvent::ImeCommit("z"), Duration::ZERO);
    let held = queue.front().expect("held");
    assert_eq!(queue.text(held.text), "z");
    assert_eq!(queue.text.len(), 1, "the buffer restarted rather than grew");
}
