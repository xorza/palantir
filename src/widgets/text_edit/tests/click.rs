use crate::{FocusPolicy, widgets::text_edit::tests::*};

#[test]
fn typing_inserts_text_when_focused() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::new();
    let id = WidgetId::from_hash("editor");

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('h'),
        repeat: false,
        physical: Key::Other,
    });
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('i'),
        repeat: false,
        physical: Key::Other,
    });

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    assert_eq!(buf, "hi");
}

#[test]
fn keystrokes_ignored_when_not_focused() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::new();

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
        physical: Key::Other,
    });

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    assert_eq!(buf, "", "unfocused TextEdit must not consume keystrokes");
    assert!(ui.focused_id().is_none());
}

#[test]
fn escape_blurs_focus() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::from("text");
    let id = WidgetId::from_hash("editor");

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(SMALL, editor_only(&mut buf));
    assert_eq!(ui.focused_id(), None);
}

#[test]
fn caret_clamps_after_external_buffer_shrink() {
    // WindowRenderer can mutate buffer between frames; if new len < cached caret,
    // `show()` must clamp at the top of the next frame instead of OOB.
    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::from("hello");

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(SMALL, editor_only(&mut buf));

    buf = String::from("hi");
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(SMALL, editor_only(&mut buf));
    assert_eq!(
        buf, "hi!",
        "clamping must keep insertion at end of shrunken buffer"
    );
}

#[test]
fn text_event_inserts_at_caret_when_focused() {
    use crate::input::keyboard::TextChunk;

    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::new();

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));

    ui.on_input(InputEvent::Text(TextChunk::new("héllo").unwrap()));
    ui.run_at_acked(SMALL, editor_only(&mut buf));
    assert_eq!(buf, "héllo");
}

#[test]
fn pointer_state_respects_pointer_left() {
    // Sanity: leaving the surface clears the click hit-test path so a
    // subsequent KeyDown to a focused TextEdit still works.
    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::new();

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::PointerLeft);
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('z'),
        repeat: false,
        physical: Key::Other,
    });

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    assert_eq!(buf, "z");
}

#[test]
fn pressed_button_does_not_route_to_textedit_under_default_policy() {
    // Default ClearOnMiss: clicking a non-focusable Button drops focus.
    let mut ui = Ui::for_test_at_text(WIDE);
    let mut buf = String::new();

    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(WidgetId::from_hash("editor")));

    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    ui.click_at(Vec2::new(200.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        None,
        "default ClearOnMiss drops focus when clicking a non-focusable Button",
    );

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    assert_eq!(buf, "");
}

#[test]
fn pressed_button_under_preserve_policy_keeps_focus() {
    let mut ui = Ui::for_test_at_text(WIDE);
    ui.set_focus_policy(FocusPolicy::PreserveOnMiss);
    let mut buf = String::new();

    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    ui.click_at(Vec2::new(200.0, 20.0));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    assert_eq!(buf, "x");
}

#[test]
fn pressed_button_pointer_jitter_does_not_steal_caret() {
    // Regression: pointer movement while NOT pressed shouldn't reset caret.
    let mut ui = Ui::for_test_at_text(WIDE);
    let mut buf = String::from("ab");

    ui.run_at_acked(WIDE, editor_only(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(WIDE, editor_only(&mut buf));

    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 20.0)));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
        physical: Key::Other,
    });

    ui.run_at_acked(WIDE, editor_only(&mut buf));
    assert_eq!(buf, "ab!");
}

#[test]
fn click_lands_caret_at_pressed_position() {
    // Mono fallback: 8 px per char @ 16 px font. With theme's default
    // 8 px left padding, x=32 → caret=3.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello world");

    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));

    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    assert_eq!(buf, "helXlo world");
}

#[test]
fn click_uses_overridden_padding() {
    // `.padding(...)` shifts both rendering and click hit-test
    // consistently. Override 24 px left → x=32 hits offset 1.
    let pad = Some(Spacing::xy(24.0, 6.0));
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello world");

    ui.run_at_acked(NARROW, editor_at(&mut buf, pad));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));

    ui.run_at_acked(NARROW, editor_at(&mut buf, pad));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(NARROW, editor_at(&mut buf, pad));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    assert_eq!(buf, "hXello world");
}

#[test]
fn drag_select_continues_past_editor_bounds() {
    // Regression: while the button is held, dragging the pointer outside
    // the editor's rect must keep extending the selection (caret rides the
    // clamped hit) and must NOT drop the drag anchor. Before the fix the
    // drag-select gated on `pressed` (hover-gated), which flipped false the
    // instant the pointer left the rect — freezing selection, clearing the
    // anchor, and (on re-entry) re-latching as a fresh press that wiped the
    // selection. Now it gates on the capture-based, rect-independent `held`.
    // Mono fallback (8 px/char) for predictable hit math.
    let ed_id = WidgetId::from_hash("drag-ed");
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("drag-ed"))
                .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello world"); // 11 bytes

    // Record once so the editor's rect is known to the next frame's hit-test.
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));

    // Press inside: caret lands mid-text and the anchor latches there.
    ui.press_at(Vec2::new(22.0, 20.0));
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    let anchor = ui.state_mut::<TextEditState>(ed_id).caret;
    assert!(
        anchor > 0 && anchor < buf.len(),
        "press should land mid-text (room to extend both ways), got {anchor}",
    );
    {
        let st = ui.state_mut::<TextEditState>(ed_id);
        assert_eq!(st.drag_anchor, Some(anchor));
        assert_eq!(st.selection, None, "a single press selects nothing yet");
    }

    // Drag far RIGHT, way past the editor's right edge. Selection extends
    // to end-of-text; the anchor is preserved.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(4000.0, 20.0)));
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    {
        let st = ui.state_mut::<TextEditState>(ed_id);
        assert_eq!(
            st.caret,
            buf.len(),
            "caret rides to the clamped end past the right edge"
        );
        assert_eq!(
            st.selection,
            Some(anchor),
            "selection extends from the anchor — not lost"
        );
        assert_eq!(
            st.drag_anchor,
            Some(anchor),
            "anchor survives the out-of-bounds drag"
        );
    }

    // Drag far LEFT, past the left edge. Caret clamps to 0; the anchor is
    // still latched so the selection just flips direction.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(-2000.0, 20.0)));
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    {
        let st = ui.state_mut::<TextEditState>(ed_id);
        assert_eq!(st.caret, 0, "caret clamps to 0 past the left edge");
        assert_eq!(
            st.selection,
            Some(anchor),
            "still selected — the anchor held"
        );
    }

    // Pointer leaves the surface entirely mid-drag: no position this frame,
    // but the gesture is still live — anchor and selection must persist.
    ui.on_input(InputEvent::PointerLeft);
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    {
        let st = ui.state_mut::<TextEditState>(ed_id);
        assert_eq!(
            st.selection,
            Some(anchor),
            "off-surface must not drop the selection"
        );
        assert_eq!(
            st.drag_anchor,
            Some(anchor),
            "off-surface must not drop the anchor"
        );
    }

    // Release ends the gesture: the anchor drops, the selection persists.
    ui.release_left();
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    {
        let st = ui.state_mut::<TextEditState>(ed_id);
        assert_eq!(st.selection, Some(anchor), "selection survives release");
        assert_eq!(st.drag_anchor, None, "release clears the drag anchor");
    }
}

#[test]
fn two_textedits_only_one_focused_at_a_time() {
    let mut ui = Ui::for_test_at_text(WIDE);
    let mut a = String::new();
    let mut b = String::new();
    let id_a = WidgetId::from_hash("a");
    let id_b = WidgetId::from_hash("b");

    let body = |ui: &mut Ui, a: &mut String, b: &mut String| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(a)
                .id(WidgetId::from_hash("a"))
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
            TextEdit::new(b)
                .id(WidgetId::from_hash("b"))
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    };

    ui.run_at_acked(WIDE, |ui| body(ui, &mut a, &mut b));
    ui.click_at(Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id_a));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('1'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(WIDE, |ui| body(ui, &mut a, &mut b));
    assert_eq!(a, "1");
    assert_eq!(b, "");

    ui.click_at(Vec2::new(250.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id_b));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('2'),
        repeat: false,
        physical: Key::Other,
    });
    ui.run_at_acked(WIDE, |ui| body(ui, &mut a, &mut b));
    assert_eq!(a, "1", "A's buffer untouched once focus moved to B");
    assert_eq!(b, "2");
}

#[test]
fn select_all_on_focus_gates_on_the_flag() {
    // Focus handed over programmatically (no pointer press) — the DragValue
    // click-to-edit handoff. With the flag the buffer is selected so the first
    // keystroke replaces it; without it, focus leaves the selection untouched.
    let mut ui = Ui::for_test_at_text(WIDE);
    let mut on = String::from("1.985");
    let mut off = String::from("42.0");
    let on_id = WidgetId::from_hash("sa-on");
    let off_id = WidgetId::from_hash("sa-off");

    let render = |ui: &mut Ui, on: &mut String, off: &mut String| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(on)
                .id(on_id)
                .select_all_on_focus()
                .size((Sizing::Fixed(120.0), Sizing::Fixed(40.0)))
                .show(ui);
            TextEdit::new(off)
                .id(off_id)
                .size((Sizing::Fixed(120.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    };

    ui.run_at_acked(WIDE, |ui| render(ui, &mut on, &mut off));
    ui.request_focus(Some(on_id));
    ui.run_at_acked(WIDE, |ui| render(ui, &mut on, &mut off));
    {
        let st = ui.state_mut::<TextEditState>(on_id);
        assert_eq!(
            st.selection,
            Some(0),
            "flag on: focus selects from the start"
        );
        assert_eq!(
            st.caret,
            "1.985".len(),
            "flag on: ...to the end of the buffer"
        );
    }

    ui.request_focus(Some(off_id));
    ui.run_at_acked(WIDE, |ui| render(ui, &mut on, &mut off));
    assert_eq!(
        ui.state_mut::<TextEditState>(off_id).selection,
        None,
        "flag off: focus leaves the selection untouched"
    );
}
