use crate::widgets::text_edit::tests::*;

#[derive(Debug)]
struct SelectionCase {
    label: &'static str,
    buf: &'static str,
    caret: usize,
    selection: Option<usize>,
    key: KeyPress,
    want_buf: &'static str,
    want_caret: usize,
    want_selection: Option<usize>,
}

#[test]
fn selection_state_transitions() {
    let cases: &[SelectionCase] = &[
        SelectionCase {
            label: "shift_right_latches",
            buf: "hello",
            caret: 0,
            selection: None,
            key: shift(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 1,
            want_selection: Some(0),
        },
        SelectionCase {
            label: "shift_right_extends",
            buf: "hello",
            caret: 1,
            selection: Some(0),
            key: shift(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 2,
            want_selection: Some(0),
        },
        SelectionCase {
            label: "shift_left_collapses_back_to_anchor",
            buf: "hello",
            caret: 1,
            selection: Some(0),
            key: shift(Key::ArrowLeft),
            want_buf: "hello",
            want_caret: 0,
            want_selection: None,
        },
        SelectionCase {
            label: "right_collapses_to_end",
            buf: "hello",
            caret: 1,
            selection: Some(4),
            key: press(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 4,
            want_selection: None,
        },
        SelectionCase {
            label: "left_collapses_to_start",
            buf: "hello",
            caret: 4,
            selection: Some(1),
            key: press(Key::ArrowLeft),
            want_buf: "hello",
            want_caret: 1,
            want_selection: None,
        },
        SelectionCase {
            label: "shift_home_extends_to_zero",
            buf: "hello",
            caret: 3,
            selection: None,
            key: shift(Key::Home),
            want_buf: "hello",
            want_caret: 0,
            want_selection: Some(3),
        },
        SelectionCase {
            label: "shift_end_extends_to_len",
            buf: "hello",
            caret: 2,
            selection: None,
            key: shift(Key::End),
            want_buf: "hello",
            want_caret: 5,
            want_selection: Some(2),
        },
        SelectionCase {
            label: "home_collapses",
            buf: "hello",
            caret: 4,
            selection: Some(1),
            key: press(Key::Home),
            want_buf: "hello",
            want_caret: 0,
            want_selection: None,
        },
        SelectionCase {
            label: "char_replaces_selection",
            buf: "hello",
            caret: 1,
            selection: Some(4),
            key: press(Key::Char('X')),
            want_buf: "hXo",
            want_caret: 2,
            want_selection: None,
        },
        SelectionCase {
            label: "backspace_deletes_selection",
            buf: "hello",
            caret: 4,
            selection: Some(1),
            key: press(Key::Backspace),
            want_buf: "ho",
            want_caret: 1,
            want_selection: None,
        },
        SelectionCase {
            label: "delete_deletes_selection",
            buf: "hello",
            caret: 1,
            selection: Some(4),
            key: press(Key::Delete),
            want_buf: "ho",
            want_caret: 1,
            want_selection: None,
        },
        SelectionCase {
            label: "ctrl_a_selects_all",
            buf: "hello",
            caret: 2,
            selection: None,
            key: ctrl_press(Key::Char('a')),
            want_buf: "hello",
            want_caret: 5,
            want_selection: Some(0),
        },
        SelectionCase {
            label: "ctrl_a_on_empty_noop",
            buf: "",
            caret: 0,
            selection: None,
            key: ctrl_press(Key::Char('a')),
            want_buf: "",
            want_caret: 0,
            want_selection: None,
        },
        // two-stage escape: first press collapses, leaves caret put
        SelectionCase {
            label: "escape_collapses_first",
            buf: "hello",
            caret: 4,
            selection: Some(1),
            key: press(Key::Escape),
            want_buf: "hello",
            want_caret: 4,
            want_selection: None,
        },
    ];
    for c in cases {
        let mut s = String::from(c.buf);
        let mut state = EditState {
            caret: c.caret,
            selection: c.selection,
            ..Default::default()
        };
        apply_key(&mut s, &mut state, c.key);
        assert_eq!(s, c.want_buf, "case: {} buffer", c.label);
        assert_eq!(state.caret, c.want_caret, "case: {} caret", c.label);
        assert_eq!(
            state.selection, c.want_selection,
            "case: {} selection",
            c.label
        );
    }
}
