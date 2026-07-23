use crate::widgets::text_edit::tests::*;

#[test]
fn multiline_enter_inserts_newline() {
    let mut ui = Ui::for_test_at_text(UVec2::new(300, 160));
    let mut buf = String::from("abc");
    let ed_id = WidgetId::from_hash("ml-ed");
    // Focus + caret after "abc".
    ui.request_focus(Some(ed_id));
    {
        let st = ui.state_mut::<TextEditState>(ed_id);
        st.edit.caret = 3;
    }
    ui.run_at(UVec2::new(300, 160), multiline_editor(&mut buf));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Enter,
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at(UVec2::new(300, 160), multiline_editor(&mut buf));
    assert_eq!(buf, "abc\n");
    let st = ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.edit.caret, 4);

    // A subsequent printable char goes on the new visual line.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('d'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at(UVec2::new(300, 160), multiline_editor(&mut buf));
    assert_eq!(buf, "abc\nd");
}

#[test]
fn single_line_enter_does_not_insert_newline() {
    let mut text = String::from("abc");
    let mut state = EditState {
        caret: 3,
        ..EditState::default()
    };
    apply_key(&mut text, &mut state, press(Key::Enter));
    assert_eq!(text, "abc", "single-line Enter is ignored");
    assert_eq!(state.caret, 3);
}

#[test]
fn single_line_widget_normalizes_host_newlines() {
    let mut ui = Ui::for_test_at_text(UVec2::new(300, 80));
    let mut text = String::from("first\r\nsecond\nthird");
    let mut changed = false;
    ui.run_at(UVec2::new(300, 80), |ui| {
        changed |= TextEdit::new(&mut text)
            .id(WidgetId::from_hash("single-line"))
            .size((Sizing::fixed(240.0), Sizing::fixed(40.0)))
            .show(ui)
            .changed;
    });
    assert_eq!(text, "first second third");
    assert!(changed, "normalizing host content is an observable edit");
}

/// Paste in multi-line mode preserves clipboard newlines (the
/// sanitize-on-paste behaviour is gated to single-line only).
#[test]
fn multiline_paste_keeps_newlines() {
    let mut ui = Ui::for_test_at_text(UVec2::new(300, 200));
    ui.resources.clipboard.set("line1\nline2\nline3").unwrap();
    let mut buf = String::new();
    let ed_id = WidgetId::from_hash("ml-ed");
    ui.request_focus(Some(ed_id));
    ui.run_at(UVec2::new(300, 200), multiline_editor(&mut buf));
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('v'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at(UVec2::new(300, 200), multiline_editor(&mut buf));
    assert_eq!(buf, "line1\nline2\nline3");
    let st = ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.edit.caret, buf.len());
}

/// Selection across hard breaks via Shift+Down: anchor at start,
/// caret moves to the next line at the same x. Selection range
/// straddles the `\n`.
#[test]
fn multiline_selection_crosses_newline() {
    let mut ui = Ui::for_test_at_text(UVec2::new(300, 200));
    let mut buf = String::from("first\nsecond");
    let ed_id = WidgetId::from_hash("ml-ed");
    ui.request_focus(Some(ed_id));
    // Caret on line 1, column 3.
    {
        let st = ui.state_mut::<TextEditState>(ed_id);
        st.edit.caret = 3;
    }
    ui.run_at(UVec2::new(300, 200), multiline_editor(&mut buf));
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        shift: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::KeyDown {
        key: Key::ArrowDown,
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at(UVec2::new(300, 200), multiline_editor(&mut buf));
    let st = ui.state_mut::<TextEditState>(ed_id).clone();
    assert!(
        st.edit.selection.is_some(),
        "shift+down across newline establishes a selection",
    );
    // Anchor stays at 3, caret jumped past the \n (byte 6) onto the
    // second line.
    assert_eq!(st.edit.selection, Some(3));
    assert!(
        st.edit.caret > 6 && st.edit.caret <= buf.len(),
        "caret landed on line 2 (byte > 6), got {}",
        st.edit.caret,
    );
}
