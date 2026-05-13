use super::*;

struct SelCase {
    label: &'static str,
    buf: &'static str,
    caret: usize,
    sel: Option<usize>,
    key: KeyPress,
    want_buf: &'static str,
    want_caret: usize,
    want_sel: Option<usize>,
}

#[test]
fn selection_state_transitions() {
    let cases: &[SelCase] = &[
        // shift+arrow latches anchor + extends
        SelCase {
            label: "shift_right_latches",
            buf: "hello",
            caret: 0,
            sel: None,
            key: shift(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 1,
            want_sel: Some(0),
        },
        SelCase {
            label: "shift_right_extends",
            buf: "hello",
            caret: 1,
            sel: Some(0),
            key: shift(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 2,
            want_sel: Some(0),
        },
        SelCase {
            label: "shift_left_collapses_back_to_anchor",
            buf: "hello",
            caret: 1,
            sel: Some(0),
            key: shift(Key::ArrowLeft),
            want_buf: "hello",
            want_caret: 0,
            want_sel: None,
        },
        // plain arrows collapse selection to its bounds
        SelCase {
            label: "right_collapses_to_end",
            buf: "hello",
            caret: 1,
            sel: Some(4),
            key: press(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 4,
            want_sel: None,
        },
        SelCase {
            label: "left_collapses_to_start",
            buf: "hello",
            caret: 4,
            sel: Some(1),
            key: press(Key::ArrowLeft),
            want_buf: "hello",
            want_caret: 1,
            want_sel: None,
        },
        // home/end
        SelCase {
            label: "shift_home_extends_to_zero",
            buf: "hello",
            caret: 3,
            sel: None,
            key: shift(Key::Home),
            want_buf: "hello",
            want_caret: 0,
            want_sel: Some(3),
        },
        SelCase {
            label: "shift_end_extends_to_len",
            buf: "hello",
            caret: 2,
            sel: None,
            key: shift(Key::End),
            want_buf: "hello",
            want_caret: 5,
            want_sel: Some(2),
        },
        SelCase {
            label: "home_collapses",
            buf: "hello",
            caret: 4,
            sel: Some(1),
            key: press(Key::Home),
            want_buf: "hello",
            want_caret: 0,
            want_sel: None,
        },
        // edits replace selection
        SelCase {
            label: "char_replaces_selection",
            buf: "hello",
            caret: 1,
            sel: Some(4),
            key: press(Key::Char('X')),
            want_buf: "hXo",
            want_caret: 2,
            want_sel: None,
        },
        SelCase {
            label: "backspace_deletes_selection",
            buf: "hello",
            caret: 4,
            sel: Some(1),
            key: press(Key::Backspace),
            want_buf: "ho",
            want_caret: 1,
            want_sel: None,
        },
        SelCase {
            label: "delete_deletes_selection",
            buf: "hello",
            caret: 1,
            sel: Some(4),
            key: press(Key::Delete),
            want_buf: "ho",
            want_caret: 1,
            want_sel: None,
        },
        // ctrl+a select-all
        SelCase {
            label: "ctrl_a_selects_all",
            buf: "hello",
            caret: 2,
            sel: None,
            key: cmd_press(Key::Char('a')),
            want_buf: "hello",
            want_caret: 5,
            want_sel: Some(0),
        },
        SelCase {
            label: "ctrl_a_on_empty_noop",
            buf: "",
            caret: 0,
            sel: None,
            key: cmd_press(Key::Char('a')),
            want_buf: "",
            want_caret: 0,
            want_sel: None,
        },
        // two-stage escape: first press collapses, leaves caret put
        SelCase {
            label: "escape_collapses_first",
            buf: "hello",
            caret: 4,
            sel: Some(1),
            key: press(Key::Escape),
            want_buf: "hello",
            want_caret: 4,
            want_sel: None,
        },
    ];
    for c in cases {
        let mut s = String::from(c.buf);
        let mut state = TextEditState {
            caret: c.caret,
            selection: c.sel,
            ..Default::default()
        };
        apply_key(&mut s, &mut state, c.key);
        assert_eq!(s, c.want_buf, "case: {} buffer", c.label);
        assert_eq!(state.caret, c.want_caret, "case: {} caret", c.label);
        assert_eq!(state.selection, c.want_sel, "case: {} selection", c.label);
    }
}
