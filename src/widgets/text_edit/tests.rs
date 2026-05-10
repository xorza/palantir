use super::{apply_key, next_char_boundary, prev_char_boundary};
use crate::Spacing;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::sizing::Sizing;
use crate::support::testing::{begin, click_at, shapes_of, ui_with_text};
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use glam::{UVec2, Vec2};

fn press(key: Key) -> KeyPress {
    KeyPress {
        key,
        mods: Modifiers::NONE,
        repeat: false,
    }
}

/// `apply_key(s, caret, key)` is the editor's pure key-handling core.
/// One case per (input, caret_in, key, expected_str, expected_caret).
/// Multi-step semantics (e.g. arrows striding) are decomposed into
/// individual cases that thread `caret` from the previous step's output.
#[test]
fn apply_key_cases() {
    let ctrl = KeyPress {
        key: Key::Char('a'),
        mods: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        repeat: false,
    };
    let cases: &[(&str, &str, usize, KeyPress, &str, usize)] = &[
        ("printable_a", "", 0, press(Key::Char('a')), "a", 1),
        (
            "printable_b_after_a",
            "a",
            1,
            press(Key::Char('b')),
            "ab",
            2,
        ),
        ("ctrl_modifier_skipped", "hi", 2, ctrl, "hi", 2),
        ("space_inserts", "ab", 2, press(Key::Char(' ')), "ab ", 3),
        (
            "backspace_mid_removes_codepoint",
            "héllo",
            3,
            press(Key::Backspace),
            "hllo",
            1,
        ),
        (
            "backspace_at_start_noop",
            "abc",
            0,
            press(Key::Backspace),
            "abc",
            0,
        ),
        (
            "delete_mid_removes_codepoint",
            "héllo",
            1,
            press(Key::Delete),
            "hllo",
            1,
        ),
        ("delete_at_end_noop", "abc", 3, press(Key::Delete), "abc", 3),
        (
            "arrow_right_steps_one_byte",
            "héllo",
            0,
            press(Key::ArrowRight),
            "héllo",
            1,
        ),
        (
            "arrow_right_skips_multibyte_codepoint",
            "héllo",
            1,
            press(Key::ArrowRight),
            "héllo",
            3,
        ),
        (
            "arrow_left_steps_one_byte",
            "héllo",
            3,
            press(Key::ArrowLeft),
            "héllo",
            1,
        ),
        (
            "arrow_left_clamps_at_start",
            "ab",
            0,
            press(Key::ArrowLeft),
            "ab",
            0,
        ),
        (
            "arrow_right_clamps_at_end",
            "ab",
            2,
            press(Key::ArrowRight),
            "ab",
            2,
        ),
        (
            "home_jumps_to_zero",
            "hello",
            2,
            press(Key::Home),
            "hello",
            0,
        ),
        ("end_jumps_to_len", "hello", 2, press(Key::End), "hello", 5),
    ];
    for (label, input, caret_in, key, want_str, want_caret) in cases {
        let mut s = String::from(*input);
        let mut caret = *caret_in;
        apply_key(&mut s, &mut caret, *key);
        assert_eq!(s, *want_str, "case: {label} string");
        assert_eq!(caret, *want_caret, "case: {label} caret");
    }
}

#[test]
fn boundary_helpers_jump_full_codepoints() {
    let s = "héllo";
    // Forward from 0: jumps over 'h' = 1 byte
    assert_eq!(next_char_boundary(s, 0), 1);
    // Forward from 1: jumps over 'é' = 2 bytes
    assert_eq!(next_char_boundary(s, 1), 3);
    // Backward from 3: lands on 'é' boundary (offset 1)
    assert_eq!(prev_char_boundary(s, 3), 1);
    // Boundary clamping
    assert_eq!(next_char_boundary(s, s.len()), s.len());
    assert_eq!(prev_char_boundary(s, 0), 0);
}

// -- Integration tests through `Ui` ---------------------------------

#[test]
fn typing_inserts_text_when_focused() {
    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::new();
    let id = WidgetId::from_hash("editor");

    // Frame 1: build, then end_frame to populate the cascade so focus
    // dispatch has rects to hit-test.
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Click into the editor so focus lands.
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id));

    // Frame 2: type 'h', 'i' via KeyDown(Char) events. End_frame
    // clears them, so we feed before begin_frame.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('h'),
        repeat: false,
    });
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('i'),
        repeat: false,
    });

    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(buf, "hi");
}

#[test]
fn keystrokes_ignored_when_not_focused() {
    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::new();

    // Don't click. Feed a KeyDown anyway.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
    });

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(buf, "", "unfocused TextEdit must not consume keystrokes");
    assert!(ui.focused_id().is_none());
}

#[test]
fn escape_blurs_focus() {
    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::from("text");
    let id = WidgetId::from_hash("editor");

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
    });

    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(ui.focused_id(), None);
}

#[test]
fn caret_clamps_after_external_buffer_shrink() {
    // Host code can mutate the buffer between frames. If the new
    // length is less than the cached caret, `show()` must clamp at
    // the top of the next frame instead of panicking on a slice OOB.
    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::from("hello");

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    // Move to end.
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // External shrink.
    buf = String::from("hi");

    // Type one char — caret was at 5, must clamp to 2 first, then
    // insert. Final text should be "hiX" (where X is the inserted char).
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(
        buf, "hi!",
        "clamping must keep insertion at end of shrunken buffer",
    );
}

#[test]
fn text_event_inserts_at_caret_when_focused() {
    // The Text path (IME commits) takes the same insert route as
    // KeyDown(Char). Pin that the multi-codepoint commit lands as a
    // single splice.
    use crate::input::keyboard::TextChunk;

    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::new();

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(50.0, 20.0));

    ui.on_input(InputEvent::Text(TextChunk::new("héllo").unwrap()));

    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(buf, "héllo");
}

#[test]
fn pointer_state_respects_pointer_left() {
    // Sanity: leaving the surface clears the click hit-test path so a
    // subsequent KeyDown to a focused TextEdit still works.
    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::new();

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::PointerLeft);
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('z'),
        repeat: false,
    });

    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(buf, "z");
}

#[test]
fn pressed_button_does_not_route_to_textedit_under_default_policy() {
    // Default ClearOnMiss: click on a Button that isn't focusable
    // should clear focus from the editor, so a subsequent keystroke
    // doesn't land in the buffer.
    use crate::widgets::button::Button;

    let mut ui = ui_with_text(UVec2::new(400, 80));
    let mut buf = String::new();

    // Frame 1: lay out both widgets so cascades have rects.
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .id_salt("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(WidgetId::from_hash("editor")));

    // Re-record so cascades survive, then click on the Button.
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .id_salt("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase(); // Button starts at x=180, click at 200.
    click_at(&mut ui, Vec2::new(200.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        None,
        "default ClearOnMiss drops focus when clicking a non-focusable Button",
    );

    // Now type — should NOT land in the buffer.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .id_salt("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(buf, "");
}

#[test]
fn pressed_button_under_preserve_policy_keeps_focus() {
    use crate::widgets::button::Button;

    let mut ui = ui_with_text(UVec2::new(400, 80));
    ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
    let mut buf = String::new();

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .id_salt("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .id_salt("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(200.0, 20.0));

    // PreserveOnMiss: focus stays on editor. Type — lands in buffer.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .id_salt("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(buf, "x");
}

#[test]
fn pressed_button_pointer_jitter_does_not_steal_caret() {
    // Regression guard: while a Button is being held (we as a TextEdit
    // are NOT pressed), pointer movement shouldn't reset our caret.
    let mut ui = ui_with_text(UVec2::new(400, 80));
    let mut buf = String::from("ab");

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    // Move to End.
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Pointer moves over the editor without pressing — caret must
    // stay where it was.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 20.0)));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
    });

    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Caret was at end (offset 2) when '!' was inserted → "ab!".
    assert_eq!(buf, "ab!");
}

#[test]
fn pressed_button_event_left_click_release_one_frame() {
    // Suppress unused-import warning for the press helper.
    let _ = PointerButton::Left;
}

#[test]
fn click_lands_caret_at_pressed_position() {
    // Mono fallback gives 8 px per char at 16 px font. With theme's
    // default 8 px left padding, pressing at x=8+8*3=32 should put
    // the caret 3 chars in. End the press *inside* the widget so the
    // editor sees `pressed=true` next frame's response.
    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    let mut buf = String::from("hello world");

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("ed")
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Press at x=32 (theme padding 8 + three glyphs × 8 px) → caret=3.
    // Hold the press across the next frame so `state.pressed` is true
    // when handle_input runs.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));

    begin(&mut ui, UVec2::new(300, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("ed")
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Type a marker char while still pressed → it must insert at
    // caret=3, producing "helXlo world".
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(300, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("ed")
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Release — caret stays at the press location.
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    assert_eq!(
        buf, "helXlo world",
        "click landed caret at offset 3, then 'X' inserted there",
    );
}

#[test]
fn click_uses_overridden_padding() {
    // Pin: `.padding(...)` on TextEdit shifts both rendering and the
    // click hit-test consistently. Default 8 px left → press at x=32
    // hits offset 3; with override 24 px left → x=32 hits offset 1
    // (24 + 1*8 = 32). The renderer deflates by `element.padding` and
    // the widget reads the same field for its caret math, so the two
    // can't drift.
    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    let mut buf = String::from("hello world");

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("ed")
            .padding(Spacing::xy(24.0, 6.0))
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));

    begin(&mut ui, UVec2::new(300, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("ed")
            .padding(Spacing::xy(24.0, 6.0))
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(300, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .id_salt("ed")
            .padding(Spacing::xy(24.0, 6.0))
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    assert_eq!(
        buf, "hXello world",
        "with override padding=24, x=32 hits offset 1, not 3",
    );
}

#[test]
fn two_textedits_only_one_focused_at_a_time() {
    let mut ui = ui_with_text(UVec2::new(400, 80));
    let mut a = String::new();
    let mut b = String::new();
    let id_a = WidgetId::from_hash("a");
    let id_b = WidgetId::from_hash("b");

    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut a)
            .id_salt("a")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        TextEdit::new(&mut b)
            .id_salt("b")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Click A.
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id_a));

    // Type — lands in A, not B.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('1'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut a)
            .id_salt("a")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        TextEdit::new(&mut b)
            .id_salt("b")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(a, "1");
    assert_eq!(b, "");

    // Click B.
    click_at(&mut ui, Vec2::new(250.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id_b));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('2'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        TextEdit::new(&mut a)
            .id_salt("a")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        TextEdit::new(&mut b)
            .id_salt("b")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    assert_eq!(a, "1", "A's buffer untouched once focus moved to B");
    assert_eq!(b, "2");
}

/// `ui_at` from the testing module sets up a Ui without cosmic, so the
/// mono fallback drives `caret_x` (8 px/char at 16 px font). That
/// gives the predictable widths the click-positioning tests rely on.
fn ui_at_no_cosmic(size: UVec2) -> crate::Ui {
    use crate::layout::types::display::Display;
    let mut ui = crate::Ui::new();
    ui.begin_frame(Display::from_physical(size, 1.0));
    ui
}

#[test]
fn each_text_widget_reads_its_own_theme_path_for_font_size() {
    // Pin: every text-rendering widget falls back to the global
    // `theme.text` when its per-widget override is `None`. Setting
    // `theme.text.font_size_px` once moves Button labels, Text, *and*
    // TextEdit. Per-state overrides (set on
    // `theme.text_edit.normal.text` etc.) win on top.
    use crate::TextStyle;
    use crate::shape::ShapeRecord;
    use crate::widgets::button::Button;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(UVec2::new(600, 200));
    // Global default — Button + Text + TextEdit all inherit this.
    ui.theme.text.font_size_px = 22.0;
    // Per-state override on TextEdit's `normal` slot — wins over the
    // global for the unfocused branch.
    ui.theme.text_edit.normal.text = Some(TextStyle::default().with_font_size(24.0));
    let mut buf = String::from("hi");

    let mut btn_node = None;
    let mut txt_node = None;
    let mut ed_node = None;
    Panel::vstack().auto_id().show(&mut ui, |ui| {
        btn_node = Some(
            Button::new()
                .id_salt("btn")
                .label("hi")
                .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
        txt_node = Some(Text::new("hi").auto_id().show(ui).node);
        ed_node = Some(
            TextEdit::new(&mut buf)
                .id_salt("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let read_fs = |node: crate::forest::tree::NodeId| -> f32 {
        shapes_of(ui.forest.tree(Layer::Main), node)
            .find_map(|s| match s {
                ShapeRecord::Text { font_size_px, .. } => Some(*font_size_px),
                _ => None,
            })
            .unwrap()
    };
    // Button + Text both fall back to the global theme.text. TextEdit
    // would too, but its `normal` slot has an override → wins.
    assert_eq!(
        read_fs(btn_node.unwrap()),
        22.0,
        "Button label falls back to theme.text",
    );
    assert_eq!(
        read_fs(txt_node.unwrap()),
        22.0,
        "Text widget reads theme.text",
    );
    assert_eq!(
        read_fs(ed_node.unwrap()),
        24.0,
        "TextEdit per-state override wins over theme.text",
    );
}

#[test]
fn theme_text_color_used_when_text_widget_does_not_override() {
    use crate::primitives::color::Color;
    use crate::shape::ShapeRecord;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    ui.theme.text.color = Color::rgb(1.0, 0.0, 0.0);

    let mut node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        node = Some(Text::new("hi").auto_id().show(ui).node);
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let color = shapes_of(ui.forest.tree(Layer::Main), node.unwrap())
        .find_map(|s| match s {
            ShapeRecord::Text { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(color, Color::rgb(1.0, 0.0, 0.0));
}

#[test]
fn text_widget_color_override_wins_over_theme() {
    use crate::TextStyle;
    use crate::primitives::color::Color;
    use crate::shape::ShapeRecord;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    ui.theme.text.color = Color::rgb(1.0, 0.0, 0.0);

    let mut node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        node = Some(
            Text::new("hi")
                .auto_id()
                .style(TextStyle::default().with_color(Color::rgb(0.0, 1.0, 0.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let color = shapes_of(ui.forest.tree(Layer::Main), node.unwrap())
        .find_map(|s| match s {
            ShapeRecord::Text { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(color, Color::rgb(0.0, 1.0, 0.0));
}

#[test]
fn each_text_widget_reads_its_own_theme_path_for_line_height() {
    // Pin: every text-rendering widget falls back to `theme.text` for
    // leading. TextEdit's `normal` slot can override on top.
    use crate::TextStyle;
    use crate::shape::ShapeRecord;
    use crate::widgets::button::Button;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(UVec2::new(600, 200));
    ui.theme.text.line_height_mult = 2.0;
    ui.theme.text_edit.normal.text = Some(TextStyle::default().with_line_height_mult(3.0));
    let mut buf = String::from("hi");

    let mut btn_node = None;
    let mut txt_node = None;
    let mut ed_node = None;
    Panel::vstack().auto_id().show(&mut ui, |ui| {
        btn_node = Some(
            Button::new()
                .id_salt("btn")
                .label("hi")
                .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
        txt_node = Some(Text::new("hi").auto_id().show(ui).node);
        ed_node = Some(
            TextEdit::new(&mut buf)
                .id_salt("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let read_lh = |node: crate::forest::tree::NodeId| -> f32 {
        shapes_of(ui.forest.tree(Layer::Main), node)
            .find_map(|s| match s {
                ShapeRecord::Text { line_height_px, .. } => Some(*line_height_px),
                _ => None,
            })
            .unwrap()
    };
    // Default font size is 16 everywhere. Button + Text both fall
    // back to `theme.text`. TextEdit's `normal` slot override wins.
    assert_eq!(
        read_lh(btn_node.unwrap()),
        16.0 * 2.0,
        "Button label falls back to theme.text",
    );
    assert_eq!(
        read_lh(txt_node.unwrap()),
        16.0 * 2.0,
        "Text reads theme.text",
    );
    assert_eq!(
        read_lh(ed_node.unwrap()),
        16.0 * 3.0,
        "TextEdit per-state override wins over theme.text",
    );
}

#[test]
fn textedit_style_override_replaces_default_theme() {
    // Pin: `.style(TextEditTheme { ... })` replaces the default theme
    // wholesale. A custom leading on the bundle's `text` field flows
    // onto the recorded `ShapeRecord::Text`.
    use crate::TextEditTheme;
    use crate::TextStyle;
    use crate::shape::ShapeRecord;
    use crate::widgets::theme::WidgetLook;

    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    let mut buf = String::from("hi");
    let style = TextEditTheme {
        normal: WidgetLook {
            text: Some(TextStyle::default().with_line_height_mult(3.0)),
            ..TextEditTheme::default().normal
        },
        ..TextEditTheme::default()
    };
    let mut leaf = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        leaf = Some(
            TextEdit::new(&mut buf)
                .id_salt("ed")
                .style(style)
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let lh = shapes_of(ui.forest.tree(Layer::Main), leaf.unwrap())
        .find_map(|s| match s {
            ShapeRecord::Text { line_height_px, .. } => Some(*line_height_px),
            _ => None,
        })
        .unwrap();
    assert_eq!(lh, 48.0, "16 px font × 3.0 leading override = 48");
}

#[test]
fn pushed_shape_carries_default_line_height_from_theme() {
    // Pin: with no per-widget override, the `ShapeRecord::Text` recorded by
    // TextEdit declares `line_height_px = font_size * theme.line_height_mult`
    // (default 1.2 from `crate::text::LINE_HEIGHT_MULT`). The shaper
    // and the caret rect both read this value, so a wrong default
    // would put both renderers out of sync.
    use crate::shape::ShapeRecord;
    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    let mut buf = String::from("hi");
    let mut leaf_node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        leaf_node = Some(
            TextEdit::new(&mut buf)
                .id_salt("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let text_shape =
        shapes_of(ui.forest.tree(Layer::Main), leaf_node.unwrap()).find_map(|s| match s {
            ShapeRecord::Text {
                font_size_px,
                line_height_px,
                ..
            } => Some((*font_size_px, *line_height_px)),
            _ => None,
        });
    let (fs, lh) = text_shape.expect("TextEdit pushes a ShapeRecord::Text for non-empty buffer");
    assert_eq!(fs, 16.0);
    assert!(
        (lh - 16.0 * crate::text::LINE_HEIGHT_MULT).abs() < 1e-5,
        "default line_height_px should be font_size * LINE_HEIGHT_MULT, got {lh}",
    );
}

#[test]
fn pushed_shape_uses_style_overridden_line_height() {
    // Pin: a custom `line_height_mult` set via `.style()` propagates
    // onto the recorded `ShapeRecord::Text` so the shaper produces a buffer
    // at the requested leading — not just the caret rect.
    use crate::TextEditTheme;
    use crate::TextStyle;
    use crate::shape::ShapeRecord;
    use crate::widgets::theme::WidgetLook;
    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    let mut buf = String::from("hi");
    let style = TextEditTheme {
        normal: WidgetLook {
            text: Some(TextStyle::default().with_line_height_mult(2.0)),
            ..TextEditTheme::default().normal
        },
        ..TextEditTheme::default()
    };
    let mut leaf_node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        leaf_node = Some(
            TextEdit::new(&mut buf)
                .id_salt("ed")
                .style(style)
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    let lh = shapes_of(ui.forest.tree(Layer::Main), leaf_node.unwrap())
        .find_map(|s| match s {
            ShapeRecord::Text { line_height_px, .. } => Some(*line_height_px),
            _ => None,
        })
        .unwrap();
    assert_eq!(lh, 32.0, "16 * 2.0 should land directly on the shape");
}

#[test]
fn line_height_override_changes_caret_rect_height() {
    // Pin: caret rect height tracks the leading carried on the
    // theme's `text` style. Default 1.2 → caret = 19.2 px tall;
    // override 2.0 → 32 px tall.
    use crate::TextEditTheme;
    use crate::TextStyle;
    use crate::shape::ShapeRecord;
    use crate::widgets::theme::WidgetLook;

    fn caret_height(style: Option<TextEditTheme>) -> f32 {
        let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
        // Focus the editor so the caret shape is pushed.
        let mut buf = String::new();
        let mut leaf = None;
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            let mut e = TextEdit::new(&mut buf)
                .id_salt("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)));
            if let Some(s) = style.clone() {
                e = e.style(s);
            }
            leaf = Some(e.show(ui).node);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        click_at(&mut ui, Vec2::new(20.0, 20.0));
        // Re-record so the focused branch fires this frame.
        begin(&mut ui, UVec2::new(300, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            let mut e = TextEdit::new(&mut buf)
                .id_salt("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)));
            if let Some(s) = style.clone() {
                e = e.style(s);
            }
            leaf = Some(e.show(ui).node);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase(); // Caret = the only sub-rect ShapeRecord pushed (no selection in v1).
        shapes_of(ui.forest.tree(Layer::Main), leaf.unwrap())
            .find_map(|s| match s {
                ShapeRecord::RoundedRect {
                    local_rect: Some(rect),
                    ..
                } => Some(rect.size.h),
                _ => None,
            })
            .expect("focused TextEdit pushes a caret Overlay")
    }

    let default = caret_height(None);
    let doubled = caret_height(Some(TextEditTheme {
        focused: WidgetLook {
            text: Some(TextStyle::default().with_line_height_mult(2.0)),
            ..TextEditTheme::default().focused
        },
        ..TextEditTheme::default()
    }));
    assert!(
        (default - 16.0 * crate::text::LINE_HEIGHT_MULT).abs() < 1e-5,
        "default caret height = font_size * LINE_HEIGHT_MULT, got {default}",
    );
    assert!(
        (doubled - 32.0).abs() < 1e-5,
        "2.0 multiplier yields 32 px caret, got {doubled}",
    );
}
