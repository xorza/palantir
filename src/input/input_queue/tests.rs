use crate::input::input_queue::InputQueue;
use crate::primitives::input::ime_preedit::ImePreedit;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::interaction::wheel_delta::WheelDelta;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_text::KeyText;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::PointerButton;
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
        InputEvent::Scroll(_) | InputEvent::Pinch(_) => {
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
            &[InputEvent::Scroll(WheelDelta::notches(Vec2::Y)), shift_up],
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

    // A stream that never empties the queue: each pop leaves the read text behind until it is
    // half the buffer. "z" read with "ab" held: 1 read byte of 3 stays as "cd" joins; "ab" read
    // too: 3 read bytes of 5 go as "ef" joins, "cd" rebased to the start.
    queue.defer(InputEvent::ImeCommit("ab"), Duration::ZERO);
    queue.pop_front();
    queue.defer(InputEvent::ImeCommit("cd"), Duration::ZERO);
    assert_eq!(queue.text, "zabcd", "1 read byte of 3 stays");
    queue.pop_front();
    queue.defer(InputEvent::ImeCommit("ef"), Duration::ZERO);
    assert_eq!(queue.text, "cdef");
    let held: Vec<_> = queue
        .pending
        .iter()
        .map(|held| queue.text(held.text))
        .collect();
    assert_eq!(held, ["cd", "ef"]);
}

/// A move right behind a held move replaces it and a scroll right behind a held scroll adds to it
/// lane by lane, a pixel turn and a notched one alike, each taking the newer arrival time; any
/// other pair holds both, so a move past a held press keeps its own place.
#[test]
fn held_moves_and_scroll_deltas_merge_into_their_neighbor() {
    let ms = Duration::from_millis;
    let mut queue = InputQueue::default();
    queue.defer(InputEvent::PointerPressed(PointerButton::Left), ms(1));
    for (i, x) in [10.0, 20.0, 30.0].into_iter().enumerate() {
        queue.defer(
            InputEvent::PointerMoved(Vec2::new(x, 5.0)),
            ms(2 + i as u64),
        );
    }
    queue.defer(
        InputEvent::Scroll(WheelDelta::pixels(Vec2::new(0.0, 3.0))),
        ms(5),
    );
    queue.defer(
        InputEvent::Scroll(WheelDelta::pixels(Vec2::new(1.0, 4.0))),
        ms(6),
    );
    queue.defer(
        InputEvent::Scroll(WheelDelta {
            notches: Vec2::new(0.0, 1.0),
            lines: Vec2::new(0.0, 3.0),
            pages: Vec2::ZERO,
            ..WheelDelta::ZERO
        }),
        ms(7),
    );
    queue.defer(
        InputEvent::Scroll(WheelDelta {
            notches: Vec2::new(0.0, 2.0),
            lines: Vec2::new(0.0, 6.0),
            pages: Vec2::new(0.0, 1.0),
            ..WheelDelta::ZERO
        }),
        ms(7),
    );
    queue.defer(InputEvent::PointerMoved(Vec2::new(40.0, 5.0)), ms(8));

    // `InputEvent` derives no `PartialEq`; its `Debug` prints every field.
    let held: Vec<_> = queue.pending.iter().map(|h| (h.event, h.at)).collect();
    assert_eq!(
        format!("{held:?}"),
        format!(
            "{:?}",
            [
                (InputEvent::PointerPressed(PointerButton::Left), ms(1)),
                (InputEvent::PointerMoved(Vec2::new(30.0, 5.0)), ms(4)),
                // Each lane adds on its own: pixels 0 + 1 = 1 and 3 + 4 = 7, notches 1 + 2,
                // lines 3 + 6 and pages 0 + 1.
                (
                    InputEvent::Scroll(WheelDelta {
                        pixels: Vec2::new(1.0, 7.0),
                        notches: Vec2::new(0.0, 3.0),
                        lines: Vec2::new(0.0, 9.0),
                        pages: Vec2::new(0.0, 1.0),
                    }),
                    ms(7)
                ),
                (InputEvent::PointerMoved(Vec2::new(40.0, 5.0)), ms(8)),
            ]
        )
    );
    assert!(!queue.is_full());
}
