use crate::widgets::text_edit::history::EditKind;
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

/// Typing undoes a word at a time: "hi you" typed at once undoes to "hi " and then to "", as a
/// word after a space starts its own group; the space joins the word before it.
#[test]
fn a_typed_word_after_a_space_starts_its_own_group() {
    let mut s = String::new();
    let mut state = EditState::default();
    for c in "hi you".chars() {
        apply_key(&mut s, &mut state, press(Key::Char(c)));
    }
    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "hi ");
    apply_key(&mut s, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(s, "");
}

/// A pause closes a group: "ab" typed 10 ms apart is one group, and a "c" 300 ms after the "b",
/// the pause that closes it, starts its own; 299 ms after, it joins.
#[test]
fn a_pause_closes_the_typing_group() {
    for (gap_ms, after_undo) in [(300, "ab"), (299, "")] {
        let mut keys = FieldKeys::new(Clipboard::memory());
        let mut s = String::new();
        let mut state = EditState::default();
        for (c, at_ms) in [('a', 0), ('b', 10), ('c', 10 + gap_ms)] {
            keys.now = Duration::from_millis(at_ms);
            keys.apply(&mut s, &mut state, press(Key::Char(c)));
        }
        keys.apply(&mut s, &mut state, ctrl_press(Key::Char('z')));
        assert_eq!(s, after_undo, "a gap of {gap_ms} ms");
    }
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
    assert_eq!(state.history.redo_len(), 1);

    let mut editor = Editor::new(
        &mut s,
        &mut state,
        EditRules {
            multiline: false,
            max_chars: Some(1),
        },
        Duration::ZERO,
    );
    apply_editor_key(&mut editor, press(Key::Char('x')), NAV);
    assert!(!editor.edited());
    assert_eq!(editor.text(), "a");
    assert_eq!(editor.redo_len(), 1);

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
        &mut Editor::new(
            &mut s,
            &mut state,
            EditRules {
                multiline: false,
                max_chars: Some(1),
            },
            Duration::ZERO,
        ),
        press(Key::Backspace),
        NAV,
    );
    assert_eq!(s, "a");
    assert_eq!(state.history.undo_len(), 1);

    apply_editor_key(
        &mut Editor::new(
            &mut s,
            &mut state,
            EditRules {
                multiline: false,
                max_chars: Some(1),
            },
            Duration::ZERO,
        ),
        press(Key::Char('x')),
        NAV,
    );
    assert_eq!(s, "a");
    assert_eq!(state.history.undo_len(), 1);
    assert_eq!(state.history.last_kind(), Some(EditKind::Delete));

    apply_editor_key(
        &mut Editor::new(
            &mut s,
            &mut state,
            EditRules {
                multiline: false,
                max_chars: Some(1),
            },
            Duration::ZERO,
        ),
        press(Key::Backspace),
        NAV,
    );
    assert_eq!(s, "");
    assert_eq!(state.history.undo_len(), 1);

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
    assert_eq!(state.history.undo_len(), 1);

    text = String::from("host value");
    apply_key(&mut text, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(text, "host value", "undo must not cross a host replacement");
    assert_eq!(
        (state.history.undo_len(), state.history.redo_len()),
        (0, 0),
        "the replacement dropped the stale history"
    );

    apply_key(&mut text, &mut state, press(Key::Char('!')));
    assert_eq!(text, "hos!t value");
    apply_key(&mut text, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(text, "host value", "new edits start fresh history");
}

/// The history sees the buffer exactly: each local edit, undo and redo keep its copy equal by the
/// same splice, a host write of the same bytes is no replacement, and one changed byte, at an
/// equal length, is.
#[test]
fn the_history_sees_the_buffer_exactly() {
    let mut text = String::new();
    let mut state = EditState::default();
    for key in [
        press(Key::Char('a')),
        press(Key::Char('b')),
        press(Key::Backspace),
        ctrl_press(Key::Char('z')),
        ctrl_shift_press(Key::Char('z')),
    ] {
        apply_key(&mut text, &mut state, key);
        assert_eq!(state.seen, text, "after {key:?}");
    }
    assert_eq!(text, "a");

    // The same bytes written back by the host: the undo still reaches the redone delete.
    text = String::from("a");
    apply_key(&mut text, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(text, "ab", "a write of the same bytes keeps the history");

    // One byte changed at the same length is a replacement.
    apply_key(&mut text, &mut state, press(Key::Char('x')));
    assert_eq!(text, "abx");
    text = String::from("aby");
    apply_key(&mut text, &mut state, ctrl_press(Key::Char('z')));
    assert_eq!(text, "aby", "an equal-length replacement drops the history");
}

/// Redo is Ctrl+Shift+Z on every platform (Cmd+Shift+Z on macOS), and Ctrl+Y is no chord: the
/// undone "a" stays undone after it.
#[test]
fn redo_is_ctrl_shift_z_alone() {
    for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
        let keys = FieldKeys::on(platform);
        let mut s = String::new();
        let mut state = EditState::default();
        keys.apply(&mut s, &mut state, press(Key::Char('a')));
        keys.apply(&mut s, &mut state, ctrl_press(Key::Char('z')));
        keys.apply(&mut s, &mut state, ctrl_press(Key::Char('y')));
        assert_eq!(s, "", "{platform:?}: Ctrl+Y redoes nothing");
        keys.apply(&mut s, &mut state, ctrl_shift_press(Key::Char('z')));
        assert_eq!(s, "a", "{platform:?}: Ctrl+Shift+Z redoes");
    }
}
