use crate::common::clipboard::{Clipboard, test_support};
use crate::widgets::text_edit::tests::*;

/// Default context menu wires Cut / Copy / Paste / Clear against
/// the host buffer. Drives the menu end-to-end: right-click opens
/// it on the next frame, clicking a row mutates the buffer + the
/// clipboard, and the menu auto-closes.
#[test]
fn context_menu_cut_copy_paste_clear() {
    use crate::widgets::context_menu::ContextMenu;
    fn editor_id() -> WidgetId {
        WidgetId::from_hash("ctx-ed")
    }
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("ctx-ed"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }
    fn open_menu_and_record(h: &mut UiHarness, buf: &mut String) {
        ContextMenu::open(&mut h.ui, editor_id(), Vec2::new(20.0, 10.0));
        h.frame(|ui| body(ui, buf));
    }
    /// Click the center of the open menu's `row_idx`-th row (record
    /// order, separators included), then run a frame so the click is
    /// observed by `MenuItem::show`. Rows are the menu body's direct
    /// children, so their arranged rects are read straight off the tree
    /// — a fixed row pitch would silently start clicking the neighbour
    /// the moment the theme's row padding moved.
    fn click_menu_row(h: &mut UiHarness, buf: &mut String, row_idx: usize) {
        let body_id = editor_id().with("body");
        let tree = &h.ui.forest.trees[Layer::Popup];
        let body_idx = tree
            .records
            .widget_id()
            .iter()
            .position(|id| *id == body_id)
            .expect("context menu body recorded");
        let ends = tree.records.subtree_end();
        let body_end = ends[body_idx].end() as usize;
        let rects = &h.ui.layout[Layer::Popup].rect;
        let mut row = body_idx + 1;
        for _ in 0..row_idx {
            row = ends[row].end() as usize;
            assert!(row < body_end, "menu has no row {row_idx}");
        }
        h.click_at(rects[row].center());
        h.frame(|ui| body(ui, buf));
    }

    // Seed: buffer with text, select "ell" (caret=4, anchor=1).
    let mut h = ui_at_no_cosmic(SMALL);
    h.set_clipboard_text("");
    let mut buf = String::from("hello");
    h.frame(|ui| body(ui, &mut buf));
    {
        let st = h.ui.state_mut::<TextEditState>(editor_id());
        st.edit.caret = 4;
        st.edit.selection = Some(1);
    }

    // Copy → clipboard holds "ell", buffer unchanged. Menu closes
    // on click.
    open_menu_and_record(&mut h, &mut buf);
    click_menu_row(&mut h, &mut buf, 1); // row 1 == Copy
    assert_eq!(buf, "hello", "copy doesn't mutate the buffer");
    assert_eq!(h.clipboard_text(), "ell");
    assert!(
        !ContextMenu::is_open(&h.ui, editor_id()),
        "item click auto-closes menu",
    );

    // Cut → buffer drops "ell", caret collapses to selection start.
    {
        let st = h.ui.state_mut::<TextEditState>(editor_id());
        st.edit.caret = 4;
        st.edit.selection = Some(1);
    }
    open_menu_and_record(&mut h, &mut buf);
    click_menu_row(&mut h, &mut buf, 0); // row 0 == Cut
    assert_eq!(buf, "ho", "cut removes the selection");
    assert_eq!(h.clipboard_text(), "ell");
    let st = h.ui.state_mut::<TextEditState>(editor_id()).clone();
    assert_eq!(st.edit.caret, 1);
    assert_eq!(st.edit.selection, None);

    // Paste at caret → "h" + "ell" + "o" = "hello".
    open_menu_and_record(&mut h, &mut buf);
    click_menu_row(&mut h, &mut buf, 2); // row 2 == Paste
    assert_eq!(buf, "hello", "paste inserts clipboard at caret");
    let st = h.ui.state_mut::<TextEditState>(editor_id()).clone();
    assert_eq!(st.edit.caret, 4, "caret advances past pasted text");

    // Clear → buffer wiped, caret reset. Row 3 is the separator,
    // row 4 is Select All, and row 5 is Clear.
    open_menu_and_record(&mut h, &mut buf);
    click_menu_row(&mut h, &mut buf, 5);
    assert_eq!(buf, "");
    let st = h.ui.state_mut::<TextEditState>(editor_id()).clone();
    assert_eq!(st.edit.caret, 0);

    // Regression: pasting `\n`-bearing clipboard via the menu must
    // sanitize the same way the Cmd+V keypress does — otherwise the
    // single-line buffer ends up with literal line breaks it can't
    // render or hit-test. Earlier menu code lacked the sanitize call.
    h.set_clipboard_text("foo\nbar");
    open_menu_and_record(&mut h, &mut buf);
    click_menu_row(&mut h, &mut buf, 2); // Paste
    assert_eq!(
        buf, "foo bar",
        "menu Paste must sanitize newlines for single-line editor"
    );

    // Select All is menu-owned while the popup is open. The captured
    // command stream executes it once and closes the popup.
    open_menu_and_record(&mut h, &mut buf);
    h.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    });
    h.key(Key::Char('a'));
    h.frame(|ui| body(ui, &mut buf));
    let state = h.ui.state_mut::<TextEditState>(editor_id()).clone();
    assert_eq!(state.edit.sel_range(), Some(0..buf.len()));
    assert!(
        !ContextMenu::is_open(&h.ui, editor_id()),
        "Select All shortcut closes the menu",
    );
}

/// Platform clipboard shortcuts — only the *platform-primary*
/// command modifier triggers (Cmd on macOS, Ctrl elsewhere); the
/// other does not. Sweeps copy/cut/paste through one keypress shape
/// per platform.
#[test]
fn clipboard_shortcuts_apply_keypresses() {
    let clipboard = Clipboard::default();

    // Primary command modifier (`Modifiers::ctrl` is platform-
    // normalized — Cmd on macOS, Ctrl elsewhere).
    fn primary(c: char) -> KeyPress {
        KeyPress {
            key: Key::Char(c),
            mods: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            repeat: false,
            physical: Key::Other,
        }
    }

    // A non-command modifier — must NOT trigger clipboard shortcuts.
    fn non_primary(c: char) -> KeyPress {
        KeyPress {
            key: Key::Char(c),
            mods: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            repeat: false,
            physical: Key::Other,
        }
    }

    clipboard.set("").unwrap();
    let mut text = String::from("hello");
    let mut state = EditState {
        caret: 4,
        selection: Some(1),
        ..EditState::default()
    };

    // Copy: clipboard ← "ell", buffer unchanged.
    apply_key_with_clipboard(&mut text, &mut state, primary('c'), &clipboard);
    assert_eq!(text, "hello");
    assert_eq!(clipboard.get(), "ell");

    // Cut: clipboard keeps "ell", buffer drops it, caret collapses.
    apply_key_with_clipboard(&mut text, &mut state, primary('x'), &clipboard);
    assert_eq!(text, "ho");
    assert_eq!(clipboard.get(), "ell");
    assert_eq!(state.caret, 1);
    assert_eq!(state.selection, None);

    // Paste: insert clipboard at caret → "hello".
    apply_key_with_clipboard(&mut text, &mut state, primary('v'), &clipboard);
    assert_eq!(text, "hello");
    assert_eq!(state.caret, 4);

    // Non-primary modifier must NOT trigger any clipboard action.
    // (On macOS, raw Ctrl+C is not Copy; on Win/Linux, Super+C is
    // not Copy.) Reset state and verify a no-op.
    clipboard.set("CLIP").unwrap();
    let mut text2 = String::from("hello");
    let mut state2 = EditState {
        caret: 4,
        selection: Some(1),
        ..EditState::default()
    };
    apply_key_with_clipboard(&mut text2, &mut state2, non_primary('c'), &clipboard);
    assert_eq!(clipboard.get(), "CLIP", "non-primary must not copy");
    apply_key_with_clipboard(&mut text2, &mut state2, non_primary('v'), &clipboard);
    assert_eq!(text2, "hello", "non-primary must not paste");

    let rejecting = test_support::rejecting();
    let mut rejected_text = String::from("hello");
    let mut rejected_state = EditState {
        caret: 4,
        selection: Some(1),
        ..EditState::default()
    };
    apply_key_with_clipboard(
        &mut rejected_text,
        &mut rejected_state,
        primary('x'),
        &rejecting,
    );
    assert_eq!(rejected_text, "hello");
    assert_eq!(rejected_state.caret, 4);
    assert_eq!(rejected_state.selection, Some(1));
    assert!(rejected_state.undo.is_empty());
}

/// Paste of multi-line clipboard content collapses every newline run
/// (`\n`, `\r`, `\r\n`, repeated breaks) into a single space — the
/// single-line buffer can't render or hit-test newlines so they get
/// scrubbed at the input boundary. Pinning both the menu Paste and
/// the Cmd/Ctrl+V shortcut.
#[test]
fn paste_strips_newlines() {
    use crate::widgets::text_edit::unicode::sanitize_single_line;
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

    // End-to-end via Cmd+V (Ctrl+V on non-Mac): a multi-line
    // clipboard string lands in the buffer as a single
    // space-separated line.
    let clipboard = Clipboard::default();
    clipboard.set("first\r\nsecond\nthird").unwrap();
    let mut text = String::new();
    let mut state = EditState::default();
    apply_key_with_clipboard(
        &mut text,
        &mut state,
        ctrl_press(Key::Char('v')),
        &clipboard,
    );
    assert_eq!(text, "first second third");
    assert_eq!(state.caret, text.len());
}

/// `ctrl+c` etc. should NOT also insert the character — confirms the
/// shortcut branch suppresses the printable-char insert path.
#[test]
fn clipboard_shortcut_does_not_insert_char() {
    let clipboard = Clipboard::default();
    clipboard.set("").unwrap();

    let mut text = String::from("ab");
    let mut state = EditState {
        caret: 2,
        ..EditState::default()
    };
    apply_key_with_clipboard(
        &mut text,
        &mut state,
        KeyPress {
            key: Key::Char('c'),
            mods: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            repeat: false,
            physical: Key::Other,
        },
        &clipboard,
    );
    assert_eq!(text, "ab", "primary+c without a selection is a no-op");
    assert_eq!(state.caret, 2);
}

/// Right-click on the editor opens the menu — pins the secondary-
/// click → `ContextMenu::attach` wiring.
#[test]
fn secondary_click_opens_text_edit_menu() {
    use crate::widgets::context_menu::ContextMenu;
    let editor_id = WidgetId::from_hash("ctx-ed-sec");
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("ctx-ed-sec"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }

    let mut h = ui_at_no_cosmic(SMALL);
    let mut buf = String::from("hi");
    h.frame(|ui| body(ui, &mut buf));
    assert!(!ContextMenu::is_open(&h.ui, editor_id));

    h.right_click_at(Vec2::new(40.0, 20.0));
    h.frame(|ui| body(ui, &mut buf));
    assert!(
        ContextMenu::is_open(&h.ui, editor_id),
        "secondary click on TextEdit opens its default menu",
    );
}

#[test]
fn open_menu_exclusively_owns_ordered_edit_shortcuts() {
    use crate::widgets::context_menu::ContextMenu;

    let a_id = WidgetId::from_hash("focused-editor");
    let b_id = WidgetId::from_hash("menu-editor");
    let mut h = ui_at_no_cosmic(UVec2::new(400, 120));
    let mut a = String::from("focused");
    let mut b = String::from("menu");
    let body = |ui: &mut Ui, a: &mut String, b: &mut String| {
        Panel::vstack().auto_id().show(ui, |ui| {
            TextEdit::new(a)
                .id(a_id)
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
            TextEdit::new(b)
                .id(b_id)
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    };
    h.frame(|ui| body(ui, &mut a, &mut b));
    h.request_focus(Some(a_id));
    {
        let state = h.ui.state_mut::<TextEditState>(a_id);
        state.edit.caret = a.len();
        state.edit.selection = Some(0);
    }
    ContextMenu::open(&mut h.ui, b_id, Vec2::new(200.0, 20.0));
    h.frame(|ui| {
        body(ui, &mut a, &mut b);
    });

    h.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    });
    for key in [Key::Char('a'), Key::Char('x')] {
        h.key(key);
    }
    h.frame(|ui| {
        body(ui, &mut a, &mut b);
    });

    assert_eq!(
        a, "focused",
        "the focused editor must not see menu commands"
    );
    assert_eq!(b, "", "Select All then Cut must execute in arrival order");
    assert_eq!(h.clipboard_text(), "menu");
    assert!(!ContextMenu::is_open(&h.ui, b_id));
}
