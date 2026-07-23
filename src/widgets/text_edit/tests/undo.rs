use crate::common::hash;
use crate::widgets::text_edit::model::EditKind;
use crate::widgets::text_edit::tests::*;

#[test]
fn undo_redo_round_trips_typed_chars() {
    let mut s = String::new();
    let mut state = EditState::default();
    for c in ['a', 'b', 'c'] {
        apply_key(&mut s, &mut state, press(Key::Char(c)));
    }
    assert_eq!(s, "abc");
    assert_eq!(state.caret, 3);

    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "", "consecutive typing coalesces into one undo group");
    assert_eq!(state.caret, 0);

    apply_key(&mut s, &mut state, ctrl_shift_press(Key::Char('z')));
    assert_eq!(s, "abc", "redo restores the coalesced edit");
    assert_eq!(state.caret, 3);
}

#[test]
fn arrow_breaks_typing_coalesce_into_two_undo_groups() {
    let mut s = String::new();
    let mut state = EditState::default();
    apply_key(&mut s, &mut state, press(Key::Char('a')));
    apply_key(&mut s, &mut state, press(Key::Char('b')));
    apply_key(&mut s, &mut state, press(Key::ArrowLeft));
    apply_key(&mut s, &mut state, press(Key::Char('x')));
    assert_eq!(s, "axb");

    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "ab", "first undo drops the post-arrow insert only");

    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "", "second undo drops the pre-arrow typing group");
}

#[test]
fn undo_restores_selection_after_delete() {
    let mut s = String::from("hello");
    let mut state = EditState {
        caret: 4,
        selection: Some(1),
        ..Default::default()
    };
    apply_key(&mut s, &mut state, press(Key::Backspace));
    assert_eq!(s, "ho");
    assert_eq!(state.caret, 1);
    assert_eq!(state.selection, None);

    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "hello");
    assert_eq!(state.caret, 4);
    assert_eq!(state.selection, Some(1));
}

#[test]
fn redo_stack_changes_only_on_real_edit() {
    let mut s = String::new();
    let mut state = EditState::default();
    apply_key(&mut s, &mut state, press(Key::Char('a')));
    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "");
    apply_key(&mut s, &mut state, press(Key::Char('b')));
    apply_key(&mut s, &mut state, ctrl_shift_press(Key::Char('z')));
    assert_eq!(s, "b", "redo after a new edit is a no-op");

    let mut s = String::from("a");
    let mut state = EditState {
        caret: 1,
        ..Default::default()
    };
    apply_key(&mut s, &mut state, press(Key::Backspace));
    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "a");
    assert_eq!(state.redo.len(), 1);

    let mut editor = Editor::new(&mut s, &mut state, false, Some(1));
    apply_editor_key(&mut editor, press(Key::Char('x')));
    assert!(!editor.edited);
    assert_eq!(editor.text, "a");
    assert_eq!(editor.state.redo.len(), 1);

    apply_key(&mut s, &mut state, ctrl_shift_press(Key::Char('z')));
    assert_eq!(s, "", "redo survives a rejected capped insertion");
}

#[test]
fn rejected_capped_insert_preserves_delete_coalescing() {
    let mut s = String::from("ab");
    let mut state = EditState {
        caret: 2,
        ..Default::default()
    };

    apply_editor_key(
        &mut Editor::new(&mut s, &mut state, false, Some(1)),
        press(Key::Backspace),
    );
    assert_eq!(s, "a");
    assert_eq!(state.undo.len(), 1);

    apply_editor_key(
        &mut Editor::new(&mut s, &mut state, false, Some(1)),
        press(Key::Char('x')),
    );
    assert_eq!(s, "a");
    assert_eq!(state.undo.len(), 1);
    assert_eq!(state.last_edit_kind, Some(EditKind::Delete));

    apply_editor_key(
        &mut Editor::new(&mut s, &mut state, false, Some(1)),
        press(Key::Backspace),
    );
    assert_eq!(s, "");
    assert_eq!(state.undo.len(), 1);

    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "ab", "both deletes remain one undo group");
}

#[test]
fn external_replacement_discards_unrelated_history() {
    let mut text = String::new();
    let mut state = EditState::default();
    for c in ['a', 'b', 'c'] {
        apply_key(&mut text, &mut state, press(Key::Char(c)));
    }
    assert_eq!(state.undo.len(), 1);

    text = String::from("host value");
    state.observe_text_hash(hash::hash_str(&text));
    assert!(state.undo.is_empty(), "render observation drops stale undo");
    assert!(state.redo.is_empty(), "render observation drops stale redo");
    apply_key(&mut text, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(text, "host value", "undo must not cross a host replacement");

    apply_key(&mut text, &mut state, press(Key::Char('!')));
    assert_eq!(text, "hos!t value");
    apply_key(&mut text, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(text, "host value", "new edits start fresh history");
}
