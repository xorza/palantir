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
fn unrouted_keyboard_input_is_not_delivered_after_focus_changes() {
    use crate::input::keyboard::TextChunk;

    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::from("seed");
    let id = WidgetId::from_hash("editor");

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    assert!(ui.focused_id().is_none());
    assert!(
        !ui.on_input(InputEvent::KeyDown {
            key: Key::Escape,
            repeat: false,
            physical: Key::Other,
        })
        .requests_repaint,
    );
    assert!(
        !ui.on_input(InputEvent::Text(TextChunk::new("stale").unwrap()))
            .requests_repaint,
    );

    ui.click_at(Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id));
    ui.run_at_acked(SMALL, editor_only(&mut buf));

    assert_eq!(buf, "seed", "unfocused text must be discarded on arrival");
    assert_eq!(
        ui.focused_id(),
        Some(id),
        "unfocused Escape must not blur a later focus target",
    );
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
    // WindowDriver can mutate buffer between frames; if new len < cached caret,
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
                .size((Sizing::fixed(280.0), Sizing::fixed(40.0)))
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
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
            TextEdit::new(b)
                .id(WidgetId::from_hash("b"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
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
                .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                .show(ui);
            TextEdit::new(off)
                .id(off_id)
                .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
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

#[test]
fn caret_click_is_scale_invariant_under_zoom() {
    use crate::primitives::transform::TranslateScale;

    // Clicking the same fraction of the field must land the caret on the same
    // glyph whether the canvas is zoomed or not: the click arrives in surface
    // (post-transform) space and must be de-scaled before hit-testing glyphs.
    fn caret_at_scale(scale: f32) -> usize {
        let mut ui = Ui::for_test_at_text(WIDE);
        let mut buf = String::from("abcdefghij");
        let id = WidgetId::from_hash("scaled-ed");
        let render = |ui: &mut Ui, buf: &mut String| {
            Panel::zstack()
                .id(WidgetId::from_hash("scale-row"))
                .transform(TranslateScale::new(Vec2::ZERO, scale))
                .size((Sizing::fixed(300.0), Sizing::fixed(60.0)))
                .show(ui, |ui| {
                    TextEdit::new(buf)
                        .id(id)
                        .size((Sizing::fixed(200.0), Sizing::fixed(40.0)))
                        .show(ui);
                });
        };
        ui.run_at_acked(WIDE, |ui| render(ui, &mut buf));
        // 40% into the widget's on-screen width — the same logical point at any
        // zoom, so the resulting caret byte must match.
        let rect = ui.response_for(id).rect.expect("editor laid out");
        let click = Vec2::new(
            rect.min.x + rect.size.w * 0.4,
            rect.min.y + rect.size.h * 0.5,
        );
        ui.press_at(click);
        ui.run_at_acked(WIDE, |ui| render(ui, &mut buf));
        ui.state_mut::<TextEditState>(id).caret
    }

    let full = caret_at_scale(1.0);
    let zoomed_out = caret_at_scale(0.5);
    let zoomed_in = caret_at_scale(2.0);
    assert!(
        full > 0 && full < 10,
        "click should land mid-text, got {full}"
    );
    assert_eq!(
        full, zoomed_out,
        "caret placement must be zoom-invariant (1x={full}, 0.5x={zoomed_out})"
    );
    assert_eq!(
        full, zoomed_in,
        "caret placement must be zoom-invariant (1x={full}, 2x={zoomed_in})"
    );
}

#[test]
fn focus_within_follows_the_focused_widgets_ancestry() {
    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::new();
    let editor = WidgetId::from_hash("editor");
    let holder = WidgetId::from_hash("holder");
    let bystander = WidgetId::from_hash("bystander");
    let mut record = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack().id(holder).show(ui, |ui| {
                TextEdit::new(&mut buf)
                    .id(editor)
                    .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                    .show(ui);
            });
            Panel::hstack()
                .id(bystander)
                .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                .show(ui, |_| {});
        });
    };

    ui.run_at_acked(SMALL, &mut record);
    assert!(
        !ui.focus_within(holder),
        "nothing focused → no ancestor owns focus"
    );

    ui.click_at(Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(editor));
    // The focused editor is within itself and its ancestor, not
    // within a sibling or an id that was never recorded.
    assert!(ui.focus_within(editor), "self-inclusive");
    assert!(ui.focus_within(holder));
    assert!(!ui.focus_within(bystander));
    assert!(!ui.focus_within(WidgetId::from_hash("unrecorded")));
}
