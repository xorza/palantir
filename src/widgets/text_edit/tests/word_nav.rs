use super::*;

#[test]
fn word_boundary_helpers_step_word_then_skip_whitespace() {
    let cases: &[(&str, &str, usize, usize, usize)] = &[
        // label, text, from, want_next, want_prev
        ("ascii_word_then_space", "hello world", 0, 5, 0),
        ("from_inside_word", "hello world", 2, 5, 0),
        ("at_word_end_skips_to_next", "hello world", 5, 11, 0),
        ("from_space_skips_to_next_word", "hello world", 6, 11, 0),
        ("end_of_text_is_terminal", "hello world", 11, 11, 6),
        ("leading_whitespace", "  hello", 0, 7, 0),
        ("punctuation_is_own_class", "hello, world", 5, 6, 0),
        ("from_after_punct", "hello, world", 6, 12, 5),
        ("empty_string", "", 0, 0, 0),
    ];
    for (label, text, from, want_next, want_prev) in cases {
        assert_eq!(
            next_word_boundary(text, *from),
            *want_next,
            "{label}: next_word_boundary",
        );
        assert_eq!(
            prev_word_boundary(text, *from),
            *want_prev,
            "{label}: prev_word_boundary",
        );
    }
}

#[test]
fn word_range_at_picks_anchor_kind() {
    let cases: &[(&str, &str, usize, std::ops::Range<usize>)] = &[
        ("inside_word", "hello world", 3, 0..5),
        ("at_word_start", "hello world", 0, 0..5),
        ("at_word_end_picks_previous", "hello world", 5, 0..5),
        (
            "between_words_at_space_picks_word_after",
            "hello world",
            6,
            6..11,
        ),
        ("on_punctuation_selects_punct_run", "a,,b", 1, 1..3),
        ("on_whitespace_returns_empty", "  ", 1, 1..1),
        ("empty_text", "", 0, 0..0),
        ("end_of_buffer", "hello world", 11, 6..11),
    ];
    for (label, text, byte, want) in cases {
        assert_eq!(word_range_at(text, *byte), want.clone(), "{label}",);
    }
}

#[test]
fn apply_key_word_nav_cases() {
    // Word-nav modifier (Alt on macOS, Ctrl elsewhere) plus arrow.
    fn word_nav(key: Key) -> KeyPress {
        let mods = if cfg!(target_os = "macos") {
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            }
        } else {
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        };
        KeyPress {
            key,
            mods,
            repeat: false,
        }
    }
    fn word_nav_shift(key: Key) -> KeyPress {
        let mut kp = word_nav(key);
        kp.mods.shift = true;
        kp
    }

    // Single-line apply_key. label, buf, caret, key, want_caret, want_sel.
    #[allow(clippy::type_complexity)]
    let cases: &[(&str, &str, usize, KeyPress, usize, Option<usize>)] = &[
        (
            "right_jumps_to_word_end",
            "hello world",
            0,
            word_nav(Key::ArrowRight),
            5,
            None,
        ),
        (
            "right_from_word_end_skips_whitespace",
            "hello world",
            5,
            word_nav(Key::ArrowRight),
            11,
            None,
        ),
        (
            "left_jumps_to_word_start",
            "hello world",
            11,
            word_nav(Key::ArrowLeft),
            6,
            None,
        ),
        (
            "left_from_word_start_jumps_over_space",
            "hello world",
            6,
            word_nav(Key::ArrowLeft),
            0,
            None,
        ),
        (
            "shift_right_extends_selection",
            "hello world",
            0,
            word_nav_shift(Key::ArrowRight),
            5,
            Some(0),
        ),
        (
            "shift_left_extends_selection",
            "hello world",
            11,
            word_nav_shift(Key::ArrowLeft),
            6,
            Some(11),
        ),
    ];
    for (label, buf, caret, key, want_caret, want_sel) in cases {
        let mut s = String::from(*buf);
        let mut state = TextEditState {
            caret: *caret,
            ..Default::default()
        };
        apply_key(&mut s, &mut state, *key);
        assert_eq!(state.caret, *want_caret, "{label}: caret");
        assert_eq!(state.selection, *want_sel, "{label}: selection");
        assert_eq!(s, *buf, "{label}: buffer must not mutate");
    }
}
