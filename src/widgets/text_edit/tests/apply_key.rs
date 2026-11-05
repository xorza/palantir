use crate::widgets::text_edit::tests::*;

#[test]
fn apply_key_cases() {
    let cases: &[(&str, &str, usize, KeyPress, &str, usize)] = &[
        ("printable_a", "", 0, press(Key::Char('a')), "a", 1),
        (
            "printable_b_after_a",
            "a",
            1,
            press(Key::Char('b')),
            "ab",
            2,
        ),
        ("space_inserts", "ab", 2, press(Key::Char(' ')), "ab ", 3),
        (
            "backspace_mid_removes_codepoint",
            "héllo",
            3,
            press(Key::Backspace),
            "hllo",
            1,
        ),
        (
            "backspace_at_start_noop",
            "abc",
            0,
            press(Key::Backspace),
            "abc",
            0,
        ),
        (
            "delete_mid_removes_codepoint",
            "héllo",
            1,
            press(Key::Delete),
            "hllo",
            1,
        ),
        ("delete_at_end_noop", "abc", 3, press(Key::Delete), "abc", 3),
        (
            "arrow_right_steps_one_byte",
            "héllo",
            0,
            press(Key::ArrowRight),
            "héllo",
            1,
        ),
        (
            "arrow_right_skips_multibyte_codepoint",
            "héllo",
            1,
            press(Key::ArrowRight),
            "héllo",
            3,
        ),
        (
            "arrow_left_steps_one_byte",
            "héllo",
            3,
            press(Key::ArrowLeft),
            "héllo",
            1,
        ),
        (
            "arrow_left_clamps_at_start",
            "ab",
            0,
            press(Key::ArrowLeft),
            "ab",
            0,
        ),
        (
            "arrow_right_clamps_at_end",
            "ab",
            2,
            press(Key::ArrowRight),
            "ab",
            2,
        ),
        (
            "home_jumps_to_zero",
            "hello",
            2,
            press(Key::Home),
            "hello",
            0,
        ),
        ("end_jumps_to_len", "hello", 2, press(Key::End), "hello", 5),
    ];
    for (label, input, caret_in, key, want_str, want_caret) in cases {
        let mut s = String::from(*input);
        let mut state = EditState {
            caret: *caret_in,
            ..Default::default()
        };
        apply_key(&mut s, &mut state, *key);
        assert_eq!(s, *want_str, "case: {label} string");
        assert_eq!(state.caret, *want_caret, "case: {label} caret");
        assert!(
            state.selection.is_none(),
            "case: {label} must not synthesize a selection",
        );
    }
}

#[derive(Debug)]
struct ExternalReplacementCase {
    label: &'static str,
    replacement: &'static str,
    caret: usize,
    selection: Option<usize>,
    drag_anchor: Option<usize>,
    key: KeyPress,
    repaired_caret: usize,
    repaired_selection: Option<usize>,
    repaired_drag_anchor: Option<usize>,
    expected_text: &'static str,
    expected_caret: usize,
    expected_selection: Option<usize>,
}

#[test]
fn external_buffer_replacement_repairs_offsets_before_edit_and_navigation() {
    fn word_nav(key: Key) -> KeyPress {
        KeyPress::with(key, Modifiers::CTRL)
    }

    let cases = [
        // Old buffer "a" left caret byte 1; the longer "é" keeps 1 in bounds but makes it an interior UTF-8 byte.
        ExternalReplacementCase {
            label: "backspace_after_longer_multibyte_replacement",
            replacement: "é",
            caret: 1,
            selection: None,
            drag_anchor: None,
            key: press(Key::Backspace),
            repaired_caret: 0,
            repaired_selection: None,
            repaired_drag_anchor: None,
            expected_text: "é",
            expected_caret: 0,
            expected_selection: None,
        },
        ExternalReplacementCase {
            label: "delete_after_four_byte_replacement",
            replacement: "🦀x",
            caret: 3,
            selection: None,
            drag_anchor: None,
            key: press(Key::Delete),
            repaired_caret: 0,
            repaired_selection: None,
            repaired_drag_anchor: None,
            expected_text: "x",
            expected_caret: 0,
            expected_selection: None,
        },
        // "abc" and "éx" are both three bytes; both persisted anchors at byte 1 must repair before deletion.
        ExternalReplacementCase {
            label: "selection_and_drag_anchor_after_same_length_replacement",
            replacement: "éx",
            caret: 3,
            selection: Some(1),
            drag_anchor: Some(1),
            key: press(Key::Backspace),
            repaired_caret: 3,
            repaired_selection: Some(0),
            repaired_drag_anchor: Some(0),
            expected_text: "",
            expected_caret: 0,
            expected_selection: None,
        },
        ExternalReplacementCase {
            label: "delete_selection_with_repaired_caret",
            replacement: "éx",
            caret: 1,
            selection: Some(3),
            drag_anchor: None,
            key: press(Key::Delete),
            repaired_caret: 0,
            repaired_selection: Some(3),
            repaired_drag_anchor: None,
            expected_text: "",
            expected_caret: 0,
            expected_selection: None,
        },
        ExternalReplacementCase {
            label: "word_right_after_multibyte_replacement",
            replacement: "é word",
            caret: 1,
            selection: None,
            drag_anchor: None,
            key: word_nav(Key::ArrowRight),
            repaired_caret: 0,
            repaired_selection: None,
            repaired_drag_anchor: None,
            expected_text: "é word",
            expected_caret: 2,
            expected_selection: None,
        },
        ExternalReplacementCase {
            label: "word_left_after_multibyte_replacement",
            replacement: "aé",
            caret: 2,
            selection: None,
            drag_anchor: None,
            key: word_nav(Key::ArrowLeft),
            repaired_caret: 1,
            repaired_selection: None,
            repaired_drag_anchor: None,
            expected_text: "aé",
            expected_caret: 0,
            expected_selection: None,
        },
    ];

    for case in cases {
        let mut text = String::from(case.replacement);
        let mut state = EditState {
            caret: case.caret,
            selection: case.selection,
            drag_anchor: case.drag_anchor,
            ..Default::default()
        };

        state.normalize(&text);
        assert_eq!(
            state.caret, case.repaired_caret,
            "{}: repair caret",
            case.label
        );
        assert_eq!(
            state.selection, case.repaired_selection,
            "{}: repair selection",
            case.label,
        );
        assert_eq!(
            state.drag_anchor, case.repaired_drag_anchor,
            "{}: repair drag anchor",
            case.label,
        );
        assert!(
            text.is_char_boundary(state.caret),
            "{}: repaired caret boundary",
            case.label
        );
        assert!(
            state
                .selection
                .is_none_or(|offset| text.is_char_boundary(offset)),
            "{}: repaired selection boundary",
            case.label,
        );

        apply_key(&mut text, &mut state, case.key);
        assert_eq!(text, case.expected_text, "{}: edited text", case.label);
        assert_eq!(
            state.caret, case.expected_caret,
            "{}: final caret",
            case.label
        );
        assert_eq!(
            state.selection, case.expected_selection,
            "{}: final selection",
            case.label,
        );
        assert!(
            text.is_char_boundary(state.caret),
            "{}: final caret boundary",
            case.label
        );
    }
}

/// Type one char via the real (cap-aware) `apply_key`.
fn type_char(s: &mut String, state: &mut EditState, c: char, max: Option<usize>) {
    apply_editor_key(
        &mut Editor::new(
            s,
            state,
            EditRules {
                multiline: false,
                max_chars: max,
            },
            Duration::ZERO,
        ),
        press(Key::Char(c)),
    );
}

/// The cap reaches the widget through `show`: typed keys stop at `max_chars` and a paste truncates to what fits ("ab" capped at 5 takes "cd" typed, then one char of "xyz").
#[test]
fn max_chars_caps_typing_and_paste_through_show() {
    let id = WidgetId::from_hash("capped");
    let record = |ui: &mut Ui, buf: &mut String| {
        TextEdit::new(buf)
            .id(id)
            .max_chars(5)
            .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
            .show(ui);
    };
    let mut h = Harness::mono(SMALL);
    let mut buf = String::from("ab");
    h.frame(|ui| record(ui, &mut buf));
    h.click(id);
    h.frame(|ui| record(ui, &mut buf));
    h.ui_mut()
        .with_state::<TextEditState, _>(id, |_, s| s.edit.caret = 2);

    for c in ['c', 'd'] {
        h.key(Key::Char(c));
        h.frame(|ui| record(ui, &mut buf));
    }
    assert_eq!(buf, "abcd", "typing below the cap lands");

    h.set_clipboard_text("xyz");
    h.hold_modifiers(Modifiers::CTRL);
    h.key(Key::Char('v'));
    h.frame(|ui| record(ui, &mut buf));
    assert_eq!(buf, "abcdx", "the paste is cut to the one char that fits");

    h.hold_modifiers(Modifiers::NONE);
    h.key(Key::Char('q'));
    h.frame(|ui| record(ui, &mut buf));
    assert_eq!(buf, "abcdx", "at the cap a typed key is dropped");
}

#[test]
fn max_chars_caps_typed_input() {
    // Cap at 3: the first three land, the fourth is dropped.
    let mut s = String::new();
    let mut state = EditState::default();
    for c in "abcd".chars() {
        type_char(&mut s, &mut state, c, Some(3));
    }
    assert_eq!(s, "abc");
    assert_eq!(state.caret, 3);
    assert_eq!(state.char_count, Some(3));

    // At the cap, inserting in the middle is rejected outright, not stealing a slot by dropping another char.
    state.caret = 0;
    type_char(&mut s, &mut state, 'X', Some(3));
    assert_eq!(s, "abc", "insertion at the cap is dropped, not shifted");
    assert_eq!(state.caret, 0);

    state.caret = 2;
    state.selection = Some(1);
    type_char(&mut s, &mut state, 'X', Some(3));
    assert_eq!(s, "aXc", "selection deletion frees room under the cap");
    assert_eq!(state.caret, 2);
    assert_eq!(state.selection, None);
    assert_eq!(state.char_count, Some(3));

    // A cap of 0 drops the insertion whole; the selection it would replace stays.
    let mut s = String::from("abc");
    let mut state = EditState {
        caret: 2,
        selection: Some(0),
        ..EditState::default()
    };
    type_char(&mut s, &mut state, 'X', Some(0));
    assert_eq!(s, "abc", "the rejected insertion deleted nothing");
    assert_eq!((state.caret, state.selection), (2, Some(0)));
}

/// The command modifier with Home / End or Up / Down jumps to the document ends in a multi-line editor, where plain Home / End mean the visual line's.
#[test]
fn the_command_modifier_reaches_the_document_ends() {
    let mut s = String::from("one\ntwo\nthree");
    let mut state = EditState {
        caret: 5,
        ..EditState::default()
    };
    let key = |s: &mut String, state: &mut EditState, kp: KeyPress| {
        apply_editor_key(
            &mut Editor::new(
                s,
                state,
                EditRules {
                    multiline: true,
                    max_chars: None,
                },
                Duration::ZERO,
            ),
            kp,
        )
    };
    assert_eq!(
        key(&mut s, &mut state, ctrl_press(Key::End)),
        KeyOutcome::None
    );
    assert_eq!(state.caret, s.len());
    assert_eq!(
        key(&mut s, &mut state, ctrl_press(Key::Home)),
        KeyOutcome::None
    );
    assert_eq!(state.caret, 0);
    key(&mut s, &mut state, ctrl_press(Key::ArrowDown));
    assert_eq!(state.caret, s.len());
    key(&mut s, &mut state, ctrl_press(Key::ArrowUp));
    assert_eq!(state.caret, 0);
    assert_eq!(
        key(&mut s, &mut state, press(Key::End)),
        KeyOutcome::Probe(ProbeMotion {
            target: ProbeTarget::RowEnd,
            motion: CaretMove::Collapse,
        }),
        "plain End is still the visual line's",
    );
    for mods in [Modifiers::ALT, Modifiers::CTRL | Modifiers::ALT] {
        assert_eq!(
            key(&mut s, &mut state, KeyPress::with(Key::End, mods)),
            KeyOutcome::Probe(ProbeMotion {
                target: ProbeTarget::RowEnd,
                motion: CaretMove::Collapse,
            }),
            "{mods:?} does not reach the document's end",
        );
    }
}

/// One chord table on every platform, where Cmd reads as `Modifiers::CTRL` on macOS. The command
/// modifier or Alt acts on a word. Both, which are AltGr off macOS, act on one character, as no
/// modifier does. Carets in "one two": from 0 to the right, past `one` at 3, or one character at
/// 1; from 6 to the left, the start of `two` at 4, or one character at 5. In "one two three",
/// Backspace from 8 removes `two ` or the space, and Delete from 4 removes `two` or `t`.
#[test]
fn the_command_modifier_and_alt_act_on_a_word() {
    let both = Modifiers::CTRL | Modifiers::ALT;
    for (mods, right, left, backspaced, deleted) in [
        (Modifiers::CTRL, 3, 4, "one three", "one  three"),
        (Modifiers::ALT, 3, 4, "one three", "one  three"),
        (both, 1, 5, "one twothree", "one wo three"),
        (Modifiers::NONE, 1, 5, "one twothree", "one wo three"),
    ] {
        let keys = FieldKeys::new(Clipboard::memory());
        let chord = |key| KeyPress::with(key, mods);
        let mut s = String::from("one two");
        let mut state = EditState::default();
        keys.apply(&mut s, &mut state, chord(Key::ArrowRight));
        assert_eq!(state.caret, right, "{mods:?}: right from 0");
        state.caret = 6;
        keys.apply(&mut s, &mut state, chord(Key::ArrowLeft));
        assert_eq!(state.caret, left, "{mods:?}: left from 6");

        for (caret, key, want) in [(8, Key::Backspace, backspaced), (4, Key::Delete, deleted)] {
            let mut s = String::from("one two three");
            let mut state = EditState {
                caret,
                ..EditState::default()
            };
            keys.apply(&mut s, &mut state, chord(key));
            assert_eq!(s, want, "{mods:?}: {key:?} from {caret}");
        }
    }

    let mut s = String::from("one two");
    let mut state = EditState::default();
    let extend = KeyPress::with(Key::ArrowRight, Modifiers::ALT | Modifiers::SHIFT);
    FieldKeys::new(Clipboard::memory()).apply(&mut s, &mut state, extend);
    assert_eq!(
        (state.caret, state.selection),
        (3, Some(0)),
        "Shift extends"
    );
}

/// Redo is Ctrl+Shift+Z; Ctrl+Y redoes nothing.
#[test]
fn redo_is_ctrl_shift_z() {
    let keys = FieldKeys::new(Clipboard::memory());
    let mut s = String::from("a");
    let mut state = EditState {
        caret: 1,
        ..EditState::default()
    };
    keys.apply(&mut s, &mut state, press(Key::Char('b')));
    keys.apply(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "a", "premise, the typing undone");
    keys.apply(&mut s, &mut state, ctrl_press(Key::Char('y')));
    assert_eq!(s, "a", "Ctrl+Y");
    keys.apply(&mut s, &mut state, ctrl_shift_press(Key::Char('z')));
    assert_eq!(s, "ab", "Ctrl+Shift+Z");
}

#[test]
fn max_chars_counts_chars_not_bytes() {
    // Multi-byte chars: the cap is 3 scalar values, not bytes.
    let mut s = String::new();
    let mut state = EditState::default();
    for c in "éééé".chars() {
        type_char(&mut s, &mut state, c, Some(3));
    }
    assert_eq!(s, "ééé");
    assert_eq!(s.chars().count(), 3);
}

#[test]
fn escape_with_selection_does_not_blur() {
    let mut s = String::from("hello");
    let mut state = EditState {
        caret: 4,
        selection: Some(1),
        ..Default::default()
    };
    let blur = apply_key(&mut s, &mut state, press(Key::Escape));
    assert!(!blur, "first escape must collapse, not blur");
    assert_eq!(state.selection, None);
    let blur2 = apply_key(&mut s, &mut state, press(Key::Escape));
    assert!(blur2, "second escape (no selection) must blur");
}
