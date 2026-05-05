use super::{apply_key, next_char_boundary, prev_char_boundary};
use crate::Spacing;
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::sizing::Sizing;
use crate::support::testing::{begin, click_at, ui_with_text};
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
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

#[test]
fn apply_key_inserts_printable_chars() {
    let mut s = String::new();
    let mut caret = 0;
    apply_key(&mut s, &mut caret, press(Key::Char('a')));
    apply_key(&mut s, &mut caret, press(Key::Char('b')));
    assert_eq!(s, "ab");
    assert_eq!(caret, 2);
}

#[test]
fn apply_key_skips_chars_with_command_modifier() {
    // ctrl+'a' is a shortcut, not text input. v1 ignores it.
    let mut s = String::from("hi");
    let mut caret = 2;
    apply_key(
        &mut s,
        &mut caret,
        KeyPress {
            key: Key::Char('a'),
            mods: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            repeat: false,
        },
    );
    assert_eq!(s, "hi");
    assert_eq!(caret, 2);
}

#[test]
fn apply_key_space_inserts_when_no_modifier() {
    // `Key::Space` was collapsed to `Key::Char(' ')` — pin that the
    // editor still inserts a space the same way as any other char.
    let mut s = String::from("ab");
    let mut caret = 2;
    apply_key(&mut s, &mut caret, press(Key::Char(' ')));
    assert_eq!(s, "ab ");
    assert_eq!(caret, 3);
}

#[test]
fn apply_key_backspace_removes_prev_codepoint() {
    let mut s = String::from("héllo");
    let mut caret = "hé".len();
    apply_key(&mut s, &mut caret, press(Key::Backspace));
    assert_eq!(s, "hllo");
    assert_eq!(caret, "h".len());
}

#[test]
fn apply_key_backspace_at_start_is_noop() {
    let mut s = String::from("abc");
    let mut caret = 0;
    apply_key(&mut s, &mut caret, press(Key::Backspace));
    assert_eq!(s, "abc");
    assert_eq!(caret, 0);
}

#[test]
fn apply_key_delete_removes_next_codepoint() {
    let mut s = String::from("héllo");
    let mut caret = 1; // between 'h' and 'é'
    apply_key(&mut s, &mut caret, press(Key::Delete));
    assert_eq!(s, "hllo");
    assert_eq!(caret, 1);
}

#[test]
fn apply_key_delete_at_end_is_noop() {
    let mut s = String::from("abc");
    let mut caret = 3;
    apply_key(&mut s, &mut caret, press(Key::Delete));
    assert_eq!(s, "abc");
    assert_eq!(caret, 3);
}

#[test]
fn apply_key_arrows_step_codepoints() {
    let mut s = String::from("héllo");
    let mut caret = 0;
    apply_key(&mut s, &mut caret, press(Key::ArrowRight));
    assert_eq!(caret, 1);
    apply_key(&mut s, &mut caret, press(Key::ArrowRight));
    assert_eq!(caret, 3, "skipped both bytes of 'é'");
    apply_key(&mut s, &mut caret, press(Key::ArrowLeft));
    assert_eq!(caret, 1);
}

#[test]
fn apply_key_arrows_clamp_at_boundaries() {
    let mut s = String::from("ab");
    let mut caret = 0;
    apply_key(&mut s, &mut caret, press(Key::ArrowLeft));
    assert_eq!(caret, 0);
    caret = 2;
    apply_key(&mut s, &mut caret, press(Key::ArrowRight));
    assert_eq!(caret, 2);
}

#[test]
fn apply_key_home_end_jump_to_extremes() {
    let mut s = String::from("hello");
    let mut caret = 2;
    apply_key(&mut s, &mut caret, press(Key::Home));
    assert_eq!(caret, 0);
    apply_key(&mut s, &mut caret, press(Key::End));
    assert_eq!(caret, 5);
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
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

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
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

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

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    assert_eq!(buf, "", "unfocused TextEdit must not consume keystrokes");
    assert!(ui.focused_id().is_none());
}

#[test]
fn escape_blurs_focus() {
    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::from("text");
    let id = WidgetId::from_hash("editor");

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
    });

    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    assert_eq!(ui.focused_id(), None);
}

#[test]
fn caret_clamps_after_external_buffer_shrink() {
    // Host code can mutate the buffer between frames. If the new
    // length is less than the cached caret, `show()` must clamp at
    // the top of the next frame instead of panicking on a slice OOB.
    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::from("hello");

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    // Move to end.
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    // External shrink.
    buf = String::from("hi");

    // Type one char — caret was at 5, must clamp to 2 first, then
    // insert. Final text should be "hiX" (where X is the inserted char).
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

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

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();
    click_at(&mut ui, Vec2::new(50.0, 20.0));

    ui.on_input(InputEvent::Text(TextChunk::new("héllo").unwrap()));

    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    assert_eq!(buf, "héllo");
}

#[test]
fn pointer_state_respects_pointer_left() {
    // Sanity: leaving the surface clears the click hit-test path so a
    // subsequent KeyDown to a focused TextEdit still works.
    let mut ui = ui_with_text(UVec2::new(200, 80));
    let mut buf = String::new();

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    click_at(&mut ui, Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::PointerLeft);
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('z'),
        repeat: false,
    });

    begin(&mut ui, UVec2::new(200, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

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
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .with_id("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(WidgetId::from_hash("editor")));

    // Re-record so cascades survive, then click on the Button.
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .with_id("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();
    // Button starts at x=180, click at 200.
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
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .with_id("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    assert_eq!(buf, "");
}

#[test]
fn pressed_button_under_preserve_policy_keeps_focus() {
    use crate::widgets::button::Button;

    let mut ui = ui_with_text(UVec2::new(400, 80));
    ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
    let mut buf = String::new();

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .with_id("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    click_at(&mut ui, Vec2::new(50.0, 20.0));
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .with_id("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();
    click_at(&mut ui, Vec2::new(200.0, 20.0));

    // PreserveOnMiss: focus stays on editor. Type — lands in buffer.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        Button::new()
            .with_id("plain")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    assert_eq!(buf, "x");
}

#[test]
fn pressed_button_pointer_jitter_does_not_steal_caret() {
    // Regression guard: while a Button is being held (we as a TextEdit
    // are NOT pressed), pointer movement shouldn't reset our caret.
    let mut ui = ui_with_text(UVec2::new(400, 80));
    let mut buf = String::from("ab");

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    click_at(&mut ui, Vec2::new(50.0, 20.0));
    // Move to End.
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    // Pointer moves over the editor without pressing — caret must
    // stay where it was.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 20.0)));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
    });

    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("editor")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

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

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("ed")
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    // Press at x=32 (theme padding 8 + three glyphs × 8 px) → caret=3.
    // Hold the press across the next frame so `state.pressed` is true
    // when handle_input runs.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));

    begin(&mut ui, UVec2::new(300, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("ed")
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    // Type a marker char while still pressed → it must insert at
    // caret=3, producing "helXlo world".
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(300, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("ed")
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

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

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("ed")
            .padding(Spacing::xy(24.0, 6.0))
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));

    begin(&mut ui, UVec2::new(300, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("ed")
            .padding(Spacing::xy(24.0, 6.0))
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(300, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut buf)
            .with_id("ed")
            .padding(Spacing::xy(24.0, 6.0))
            .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();
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

    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut a)
            .with_id("a")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        TextEdit::new(&mut b)
            .with_id("b")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();

    // Click A.
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id_a));

    // Type — lands in A, not B.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('1'),
        repeat: false,
    });
    begin(&mut ui, UVec2::new(400, 80));
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut a)
            .with_id("a")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        TextEdit::new(&mut b)
            .with_id("b")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();
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
    Panel::hstack().show(&mut ui, |ui| {
        TextEdit::new(&mut a)
            .with_id("a")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
        TextEdit::new(&mut b)
            .with_id("b")
            .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
            .show(ui);
    });
    ui.end_frame();
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
fn theme_line_height_mult_overrides_default_for_all_text_widgets() {
    // Pin: a single `ui.theme.line_height_mult` setting flows into
    // every text-rendering widget — Button, Text, TextEdit. Without
    // this lockstep, an app that bumps the global leading would still
    // see one widget at 1.2× while others moved.
    use crate::shape::Shape;
    use crate::widgets::button::Button;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(UVec2::new(600, 200));
    ui.theme.line_height_mult = 2.0;
    let mut buf = String::from("hi");

    let mut btn_node = None;
    let mut txt_node = None;
    let mut ed_node = None;
    Panel::vstack().show(&mut ui, |ui| {
        btn_node = Some(
            Button::new()
                .with_id("btn")
                .label("hi")
                .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
        txt_node = Some(Text::new("hi").show(ui).node);
        ed_node = Some(
            TextEdit::new(&mut buf)
                .with_id("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame();

    let read_lh = |node: crate::tree::NodeId| -> f32 {
        ui.tree
            .shapes
            .slice_of(node.index())
            .iter()
            .find_map(|s| match s {
                Shape::Text { line_height_px, .. } => Some(*line_height_px),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        read_lh(btn_node.unwrap()),
        32.0,
        "Button label respects theme"
    );
    assert_eq!(
        read_lh(txt_node.unwrap()),
        32.0,
        "Text widget respects theme"
    );
    assert_eq!(
        read_lh(ed_node.unwrap()),
        32.0,
        "TextEdit (no per-widget override) respects theme",
    );
}

#[test]
fn textedit_per_widget_override_wins_over_theme() {
    // Pin: when both are set, `.line_height_mult(...)` on the builder
    // wins over `ui.theme.line_height_mult`.
    use crate::shape::Shape;

    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    ui.theme.line_height_mult = 2.0;
    let mut buf = String::from("hi");
    let mut leaf = None;
    Panel::hstack().show(&mut ui, |ui| {
        leaf = Some(
            TextEdit::new(&mut buf)
                .with_id("ed")
                .line_height_mult(3.0)
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame();
    let lh = ui
        .tree
        .shapes
        .slice_of(leaf.unwrap().index())
        .iter()
        .find_map(|s| match s {
            Shape::Text { line_height_px, .. } => Some(*line_height_px),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        lh, 48.0,
        "16 px font × 3.0 widget override = 48, ignoring theme=2.0",
    );
}

#[test]
fn pushed_shape_carries_default_line_height_from_theme() {
    // Pin: with no per-widget override, the `Shape::Text` recorded by
    // TextEdit declares `line_height_px = font_size * theme.line_height_mult`
    // (default 1.2 from `crate::text::LINE_HEIGHT_MULT`). The shaper
    // and the caret rect both read this value, so a wrong default
    // would put both renderers out of sync.
    use crate::shape::Shape;
    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    let mut buf = String::from("hi");
    let mut leaf_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        leaf_node = Some(
            TextEdit::new(&mut buf)
                .with_id("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame();

    let shapes = ui.tree.shapes.slice_of(leaf_node.unwrap().index());
    let text_shape = shapes.iter().find_map(|s| match s {
        Shape::Text {
            font_size_px,
            line_height_px,
            ..
        } => Some((*font_size_px, *line_height_px)),
        _ => None,
    });
    let (fs, lh) = text_shape.expect("TextEdit pushes a Shape::Text for non-empty buffer");
    assert_eq!(fs, 16.0);
    assert!(
        (lh - 16.0 * crate::text::LINE_HEIGHT_MULT).abs() < 1e-5,
        "default line_height_px should be font_size * LINE_HEIGHT_MULT, got {lh}",
    );
}

#[test]
fn pushed_shape_uses_per_widget_line_height_override() {
    // Pin: `.line_height_mult(2.0)` propagates onto the recorded
    // `Shape::Text` so the shaper produces a buffer at the requested
    // leading. Without this, the per-widget setter would only affect
    // the caret rect, leaving the rendered text at 1.2× — exactly the
    // leak the user pointed out.
    use crate::shape::Shape;
    let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
    let mut buf = String::from("hi");
    let mut leaf_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        leaf_node = Some(
            TextEdit::new(&mut buf)
                .with_id("ed")
                .line_height_mult(2.0)
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui)
                .node,
        );
    });
    ui.end_frame();

    let shapes = ui.tree.shapes.slice_of(leaf_node.unwrap().index());
    let lh = shapes
        .iter()
        .find_map(|s| match s {
            Shape::Text { line_height_px, .. } => Some(*line_height_px),
            _ => None,
        })
        .unwrap();
    assert_eq!(lh, 32.0, "16 * 2.0 should land directly on the shape");
}

#[test]
fn line_height_override_changes_caret_rect_height() {
    // Pin: caret rect height tracks the per-widget multiplier.
    // Default 1.2 → caret = 19.2 px tall; override 2.0 → 32 px tall.
    use crate::shape::Shape;

    fn caret_height(mult: Option<f32>) -> f32 {
        let mut ui = ui_at_no_cosmic(UVec2::new(300, 80));
        // Focus the editor so the caret shape is pushed.
        let mut buf = String::new();
        let mut leaf = None;
        Panel::hstack().show(&mut ui, |ui| {
            let mut e = TextEdit::new(&mut buf)
                .with_id("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)));
            if let Some(m) = mult {
                e = e.line_height_mult(m);
            }
            leaf = Some(e.show(ui).node);
        });
        ui.end_frame();
        click_at(&mut ui, Vec2::new(20.0, 20.0));
        // Re-record so the focused branch fires this frame.
        begin(&mut ui, UVec2::new(300, 80));
        Panel::hstack().show(&mut ui, |ui| {
            let mut e = TextEdit::new(&mut buf)
                .with_id("ed")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)));
            if let Some(m) = mult {
                e = e.line_height_mult(m);
            }
            leaf = Some(e.show(ui).node);
        });
        ui.end_frame();
        let shapes = ui.tree.shapes.slice_of(leaf.unwrap().index());
        // Caret = the only Shape::Overlay pushed (no selection in v1).
        shapes
            .iter()
            .find_map(|s| match s {
                Shape::Overlay { rect, .. } => Some(rect.size.h),
                _ => None,
            })
            .expect("focused TextEdit pushes a caret Overlay")
    }

    let default = caret_height(None);
    let doubled = caret_height(Some(2.0));
    assert!(
        (default - 16.0 * crate::text::LINE_HEIGHT_MULT).abs() < 1e-5,
        "default caret height = font_size * LINE_HEIGHT_MULT, got {default}",
    );
    assert!(
        (doubled - 32.0).abs() < 1e-5,
        "2.0 multiplier yields 32 px caret, got {doubled}",
    );
}
