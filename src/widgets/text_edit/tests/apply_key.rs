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
        let mut state = TextEditState {
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

/// Type one char via the real (cap-aware) `apply_key`.
fn type_char(s: &mut String, state: &mut TextEditState, c: char, max: Option<usize>) {
    Editor::new(s, state, false, max).apply_key(press(Key::Char(c)));
}

#[test]
fn max_chars_caps_typed_input() {
    // Cap at 3: the first three land, the fourth is dropped.
    let mut s = String::new();
    let mut state = TextEditState::default();
    for c in "abcd".chars() {
        type_char(&mut s, &mut state, c, Some(3));
    }
    assert_eq!(s, "abc");
    assert_eq!(state.caret, 3);

    // At the cap, inserting in the *middle* is rejected outright — it
    // must not steal a slot by dropping some other char.
    state.caret = 0;
    type_char(&mut s, &mut state, 'X', Some(3));
    assert_eq!(s, "abc", "insertion at the cap is dropped, not shifted");
    assert_eq!(state.caret, 0);
}

#[test]
fn max_chars_counts_chars_not_bytes() {
    // Multi-byte chars: the cap is 3 scalar values, not 3 bytes.
    let mut s = String::new();
    let mut state = TextEditState::default();
    for c in "éééé".chars() {
        type_char(&mut s, &mut state, c, Some(3));
    }
    assert_eq!(s, "ééé");
    assert_eq!(s.chars().count(), 3);
}

#[test]
fn escape_with_selection_does_not_blur() {
    let mut s = String::from("hello");
    let mut state = TextEditState {
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
