use super::InputQueue;
use crate::input::ime_preedit::ImePreedit;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::pointer::PointerButton;
use std::time::Duration;

fn key(key: Key, text: KeyText, repeat: bool) -> InputEvent<'static> {
    InputEvent::KeyDown {
        key,
        repeat,
        physical: Key::Other,
        text,
    }
}

fn typed(c: char) -> InputEvent<'static> {
    key(Key::Char(c), KeyText::from_char(c), false)
}

/// Admit `event` and note what it changes, as `InputState::apply` does.
fn admit_and_apply(queue: &mut InputQueue, event: &InputEvent<'_>) -> bool {
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

#[test]
fn a_frame_admits_one_change_per_button_and_one_command_key() {
    let left = PointerButton::Left;
    let right = PointerButton::Right;
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

/// Only what was noted holds the frame: an unlatched press leaves its release free in the same frame.
#[test]
fn an_unnoted_change_holds_nothing_back() {
    let left = PointerButton::Left;
    let queue = InputQueue::default();
    assert!(queue.admits(&InputEvent::PointerPressed(left)));
    assert!(queue.admits(&InputEvent::PointerReleased(left)));
}

/// A held event waits for `next_frame`, then pops in order, a frame's worth at a time.
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
        while let Some(held) = queue.pop_admitted() {
            admit_and_apply(&mut queue, &held.event);
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
    while let Some(held) = queue.pop_admitted() {
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
    let held = queue.pop_admitted().expect("held");
    assert_eq!(queue.text(held.text), "z");
    assert_eq!(queue.text.len(), 1, "the buffer restarted rather than grew");
}
