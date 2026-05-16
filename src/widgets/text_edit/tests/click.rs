use super::*;

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
    });
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('i'),
        repeat: false,
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
    });
    ui.run_at_acked(SMALL, editor_only(&mut buf));
    assert_eq!(ui.focused_id(), None);
}

#[test]
fn caret_clamps_after_external_buffer_shrink() {
    // Host can mutate buffer between frames; if new len < cached caret,
    // `show()` must clamp at the top of the next frame instead of OOB.
    let mut ui = Ui::for_test_at_text(SMALL);
    let mut buf = String::from("hello");

    ui.run_at_acked(SMALL, editor_only(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    ui.run_at_acked(SMALL, editor_only(&mut buf));

    buf = String::from("hi");
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
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
    });
    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    assert_eq!(buf, "");
}

#[test]
fn pressed_button_under_preserve_policy_keeps_focus() {
    let mut ui = Ui::for_test_at_text(WIDE);
    ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
    let mut buf = String::new();

    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    ui.click_at(Vec2::new(50.0, 20.0));
    ui.run_at_acked(WIDE, editor_and_button(&mut buf));
    ui.click_at(Vec2::new(200.0, 20.0));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
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
    });
    ui.run_at_acked(WIDE, editor_only(&mut buf));

    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 20.0)));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
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
    });
    ui.run_at_acked(NARROW, editor_at(&mut buf, pad));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    assert_eq!(buf, "hXello world");
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
                .id_salt("a")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
            TextEdit::new(b)
                .id_salt("b")
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
    });
    ui.run_at_acked(WIDE, |ui| body(ui, &mut a, &mut b));
    assert_eq!(a, "1");
    assert_eq!(b, "");

    ui.click_at(Vec2::new(250.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id_b));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('2'),
        repeat: false,
    });
    ui.run_at_acked(WIDE, |ui| body(ui, &mut a, &mut b));
    assert_eq!(a, "1", "A's buffer untouched once focus moved to B");
    assert_eq!(b, "2");
}
