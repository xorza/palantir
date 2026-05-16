use super::*;

/// Default context menu wires Cut / Copy / Paste / Clear against
/// the host buffer. Drives the menu end-to-end: right-click opens
/// it on the next frame, clicking a row mutates the buffer + the
/// clipboard, and the menu auto-closes.
#[test]
fn context_menu_cut_copy_paste_clear() {
    use crate::widgets::context_menu::ContextMenu;
    // Whole test holds the clipboard guard so a parallel test in
    // the lib binary can't slip between our `set`/`get` checks.
    let _cb_guard = crate::clipboard::test_serialize_guard();
    crate::clipboard::set("");

    fn editor_id() -> WidgetId {
        WidgetId::from_hash("ctx-ed")
    }
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("ctx-ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }
    fn open_menu_and_record(ui: &mut Ui, buf: &mut String) {
        ContextMenu::open(ui, editor_id(), Vec2::new(20.0, 10.0));
        ui.run_at(SMALL, |ui| body(ui, buf));
    }
    /// Click into the body element of the open menu at row-offset
    /// `(rel_x, rel_y)` from the body's top-left, then run a frame
    /// so the click is observed by `MenuItem::show`.
    fn click_menu_row(ui: &mut Ui, buf: &mut String, row_idx: usize) {
        let body_id = editor_id().with("ctx_menu_body");
        let body_rect = ui
            .layout
            .cascades
            .by_id
            .get(&body_id)
            .map(|&i| ui.layout.cascades.entries[i as usize].rect)
            .expect("context menu body recorded");
        // Theme padding ~4 px + row height ~31 px including row
        // padding. Click well inside the chosen row.
        let row_y = body_rect.min.y + 8.0 + (row_idx as f32) * 32.0;
        ui.click_at(Vec2::new(body_rect.min.x + 20.0, row_y));
        ui.run_at(SMALL, |ui| body(ui, buf));
    }

    // Seed: buffer with text, select "ell" (caret=4, anchor=1).
    let mut ui = ui_at_no_cosmic(SMALL);
    let mut buf = String::from("hello");
    ui.run_at_acked(SMALL, |ui| body(ui, &mut buf));
    {
        let st = ui.state_mut::<TextEditState>(editor_id());
        st.caret = 4;
        st.selection = Some(1);
    }

    // Copy → clipboard holds "ell", buffer unchanged. Menu closes
    // on click.
    open_menu_and_record(&mut ui, &mut buf);
    click_menu_row(&mut ui, &mut buf, 1); // row 1 == Copy
    assert_eq!(buf, "hello", "copy doesn't mutate the buffer");
    assert_eq!(crate::clipboard::get(), "ell");
    assert!(
        !ContextMenu::is_open(&ui, editor_id()),
        "item click auto-closes menu",
    );

    // Cut → buffer drops "ell", caret collapses to selection start.
    {
        let st = ui.state_mut::<TextEditState>(editor_id());
        st.caret = 4;
        st.selection = Some(1);
    }
    open_menu_and_record(&mut ui, &mut buf);
    click_menu_row(&mut ui, &mut buf, 0); // row 0 == Cut
    assert_eq!(buf, "ho", "cut removes the selection");
    assert_eq!(crate::clipboard::get(), "ell");
    let st = ui.state_mut::<TextEditState>(editor_id()).clone();
    assert_eq!(st.caret, 1);
    assert_eq!(st.selection, None);

    // Paste at caret → "h" + "ell" + "o" = "hello".
    open_menu_and_record(&mut ui, &mut buf);
    click_menu_row(&mut ui, &mut buf, 2); // row 2 == Paste
    assert_eq!(buf, "hello", "paste inserts clipboard at caret");
    let st = ui.state_mut::<TextEditState>(editor_id()).clone();
    assert_eq!(st.caret, 4, "caret advances past pasted text");

    // Clear → buffer wiped, caret reset. Row 3 is the separator;
    // row 4 is Clear in render order.
    open_menu_and_record(&mut ui, &mut buf);
    click_menu_row(&mut ui, &mut buf, 4);
    assert_eq!(buf, "");
    let st = ui.state_mut::<TextEditState>(editor_id()).clone();
    assert_eq!(st.caret, 0);

    // Regression: pasting `\n`-bearing clipboard via the menu must
    // sanitize the same way the Cmd+V keypress does — otherwise the
    // single-line buffer ends up with literal line breaks it can't
    // render or hit-test. Earlier menu code lacked the sanitize call.
    crate::clipboard::set("foo\nbar");
    open_menu_and_record(&mut ui, &mut buf);
    click_menu_row(&mut ui, &mut buf, 2); // Paste
    assert_eq!(
        buf, "foo bar",
        "menu Paste must sanitize newlines for single-line editor"
    );
}

/// Platform clipboard shortcuts — only the *platform-primary*
/// command modifier triggers (Cmd on macOS, Ctrl elsewhere); the
/// other does not. Sweeps copy/cut/paste through one keypress shape
/// per platform.
#[test]
fn clipboard_shortcuts_apply_keypresses() {
    let _cb_guard = crate::clipboard::test_serialize_guard();

    fn primary(c: char) -> KeyPress {
        let mods = if cfg!(target_os = "macos") {
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            }
        } else {
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        };
        KeyPress {
            key: Key::Char(c),
            mods,
            repeat: false,
        }
    }

    fn non_primary(c: char) -> KeyPress {
        let mods = if cfg!(target_os = "macos") {
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        } else {
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            }
        };
        KeyPress {
            key: Key::Char(c),
            mods,
            repeat: false,
        }
    }

    crate::clipboard::set("");
    let mut text = String::from("hello");
    let mut state = TextEditState {
        caret: 4,
        selection: Some(1),
        ..TextEditState::default()
    };

    // Copy: clipboard ← "ell", buffer unchanged.
    apply_key(&mut text, &mut state, primary('c'));
    assert_eq!(text, "hello");
    assert_eq!(crate::clipboard::get(), "ell");

    // Cut: clipboard keeps "ell", buffer drops it, caret collapses.
    apply_key(&mut text, &mut state, primary('x'));
    assert_eq!(text, "ho");
    assert_eq!(crate::clipboard::get(), "ell");
    assert_eq!(state.caret, 1);
    assert_eq!(state.selection, None);

    // Paste: insert clipboard at caret → "hello".
    apply_key(&mut text, &mut state, primary('v'));
    assert_eq!(text, "hello");
    assert_eq!(state.caret, 4);

    // Non-primary modifier must NOT trigger any clipboard action.
    // (On macOS, raw Ctrl+C is not Copy; on Win/Linux, Super+C is
    // not Copy.) Reset state and verify a no-op.
    crate::clipboard::set("CLIP");
    let mut text2 = String::from("hello");
    let mut state2 = TextEditState {
        caret: 4,
        selection: Some(1),
        ..TextEditState::default()
    };
    apply_key(&mut text2, &mut state2, non_primary('c'));
    assert_eq!(crate::clipboard::get(), "CLIP", "non-primary must not copy");
    apply_key(&mut text2, &mut state2, non_primary('v'));
    assert_eq!(text2, "hello", "non-primary must not paste");
}

/// Paste of multi-line clipboard content collapses every newline run
/// (`\n`, `\r`, `\r\n`, repeated breaks) into a single space — the
/// single-line buffer can't render or hit-test newlines so they get
/// scrubbed at the input boundary. Pinning both the menu Paste and
/// the Cmd/Ctrl+V shortcut.
#[test]
fn paste_strips_newlines() {
    use super::super::sanitize_single_line;
    let cases: &[(&str, &str)] = &[
        ("ab\ncd", "ab cd"),
        ("ab\rcd", "ab cd"),
        ("ab\r\ncd", "ab cd"),
        ("ab\n\ncd", "ab cd"),
        ("\nab\n", " ab "),
        ("no breaks", "no breaks"),
    ];
    for (input, expected) in cases {
        assert_eq!(
            sanitize_single_line(input),
            *expected,
            "sanitize({input:?})",
        );
    }

    // End-to-end via Cmd+V: a multi-line clipboard string lands in
    // the buffer as a single space-separated line.
    let _cb_guard = crate::clipboard::test_serialize_guard();
    crate::clipboard::set("first\r\nsecond\nthird");
    let mut text = String::new();
    let mut state = TextEditState::default();
    apply_key(
        &mut text,
        &mut state,
        KeyPress {
            key: Key::Char('v'),
            mods: Modifiers {
                meta: true,
                ..Modifiers::NONE
            },
            repeat: false,
        },
    );
    assert_eq!(text, "first second third");
    assert_eq!(state.caret, text.len());
}

/// `ctrl+c` etc. should NOT also insert the character — confirms the
/// shortcut branch suppresses the printable-char insert path.
#[test]
fn clipboard_shortcut_does_not_insert_char() {
    let _cb_guard = crate::clipboard::test_serialize_guard();
    crate::clipboard::set("");

    let mut text = String::from("ab");
    let mut state = TextEditState {
        caret: 2,
        ..TextEditState::default()
    };
    apply_key(
        &mut text,
        &mut state,
        KeyPress {
            key: Key::Char('c'),
            mods: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            repeat: false,
        },
    );
    assert_eq!(text, "ab", "ctrl+c without a selection is a no-op");
    assert_eq!(state.caret, 2);
}

/// Right-click on the editor opens the menu — pins the secondary-
/// click → `ContextMenu::attach` wiring.
#[test]
fn secondary_click_opens_text_edit_menu() {
    use crate::widgets::context_menu::ContextMenu;
    let _cb_guard = crate::clipboard::test_serialize_guard();

    let editor_id = WidgetId::from_hash("ctx-ed-sec");
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("ctx-ed-sec")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }

    let mut ui = ui_at_no_cosmic(SMALL);
    let mut buf = String::from("hi");
    ui.run_at_acked(SMALL, |ui| body(ui, &mut buf));
    assert!(!ContextMenu::is_open(&ui, editor_id));

    ui.secondary_click_at(Vec2::new(40.0, 20.0));
    ui.run_at(SMALL, |ui| body(ui, &mut buf));
    assert!(
        ContextMenu::is_open(&ui, editor_id),
        "secondary click on TextEdit opens its default menu",
    );
}
