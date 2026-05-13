use super::*;

#[test]
fn undo_redo_round_trips_typed_chars() {
    let mut s = String::new();
    let mut state = TextEditState::default();
    for c in ['a', 'b', 'c'] {
        apply_key(&mut s, &mut state, press(Key::Char(c)));
    }
    assert_eq!(s, "abc");
    assert_eq!(state.caret, 3);

    apply_key(&mut s, &mut state, cmd_press(Key::Char('z')));
    assert_eq!(s, "", "consecutive typing coalesces into one undo group");
    assert_eq!(state.caret, 0);

    apply_key(&mut s, &mut state, cmd_shift_press(Key::Char('z')));
    assert_eq!(s, "abc", "redo restores the coalesced edit");
    assert_eq!(state.caret, 3);
}

#[test]
fn arrow_breaks_typing_coalesce_into_two_undo_groups() {
    let mut s = String::new();
    let mut state = TextEditState::default();
    apply_key(&mut s, &mut state, press(Key::Char('a')));
    apply_key(&mut s, &mut state, press(Key::Char('b')));
    apply_key(&mut s, &mut state, press(Key::ArrowLeft));
    apply_key(&mut s, &mut state, press(Key::Char('x')));
    assert_eq!(s, "axb");

    apply_key(&mut s, &mut state, cmd_press(Key::Char('z')));
    assert_eq!(s, "ab", "first undo drops the post-arrow insert only");

    apply_key(&mut s, &mut state, cmd_press(Key::Char('z')));
    assert_eq!(s, "", "second undo drops the pre-arrow typing group");
}

#[test]
fn undo_restores_selection_after_delete() {
    let mut s = String::from("hello");
    let mut state = TextEditState {
        caret: 4,
        selection: Some(1),
        ..Default::default()
    };
    apply_key(&mut s, &mut state, press(Key::Backspace));
    assert_eq!(s, "ho");
    assert_eq!(state.caret, 1);
    assert_eq!(state.selection, None);

    apply_key(&mut s, &mut state, cmd_press(Key::Char('z')));
    assert_eq!(s, "hello");
    assert_eq!(state.caret, 4);
    assert_eq!(state.selection, Some(1));
}

#[test]
fn redo_stack_clears_on_fresh_edit() {
    let mut s = String::new();
    let mut state = TextEditState::default();
    apply_key(&mut s, &mut state, press(Key::Char('a')));
    apply_key(&mut s, &mut state, cmd_press(Key::Char('z')));
    assert_eq!(s, "");
    apply_key(&mut s, &mut state, press(Key::Char('b')));
    apply_key(&mut s, &mut state, cmd_shift_press(Key::Char('z')));
    assert_eq!(s, "b", "redo after a new edit is a no-op");
}
