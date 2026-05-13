use super::{TextEditState, apply_key, next_char_boundary, prev_char_boundary};
use crate::Spacing;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::sizing::Sizing;
use crate::support::testing::{
    click_at, run_at, run_at_acked, secondary_click_at, shapes_of, ui_with_text,
};
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

const SMALL: UVec2 = UVec2::new(200, 80);
const WIDE: UVec2 = UVec2::new(400, 80);
const NARROW: UVec2 = UVec2::new(300, 80);

fn editor_only(buf: &mut String) -> impl FnMut(&mut Ui) + '_ {
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("editor")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }
}

#[test]
fn apply_key_cases() {
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
        let mut state = TextEditState {
            caret: *caret_in,
            ..Default::default()
        };
        apply_key(&mut s, &mut state, *key);
        assert_eq!(s, *want_str, "case: {label} string");
        assert_eq!(state.caret, *want_caret, "case: {label} caret");
        assert!(
            state.selection.is_none(),
            "case: {label} must not synthesize a selection",
        );
    }
}

fn shift(key: Key) -> KeyPress {
    KeyPress {
        key,
        mods: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        repeat: false,
    }
}

fn ctrl(key: Key) -> KeyPress {
    KeyPress {
        key,
        mods: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        repeat: false,
    }
}

struct SelCase {
    label: &'static str,
    buf: &'static str,
    caret: usize,
    sel: Option<usize>,
    key: KeyPress,
    want_buf: &'static str,
    want_caret: usize,
    want_sel: Option<usize>,
}

#[test]
fn selection_state_transitions() {
    let cases: &[SelCase] = &[
        // shift+arrow latches anchor + extends
        SelCase {
            label: "shift_right_latches",
            buf: "hello",
            caret: 0,
            sel: None,
            key: shift(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 1,
            want_sel: Some(0),
        },
        SelCase {
            label: "shift_right_extends",
            buf: "hello",
            caret: 1,
            sel: Some(0),
            key: shift(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 2,
            want_sel: Some(0),
        },
        SelCase {
            label: "shift_left_collapses_back_to_anchor",
            buf: "hello",
            caret: 1,
            sel: Some(0),
            key: shift(Key::ArrowLeft),
            want_buf: "hello",
            want_caret: 0,
            want_sel: None,
        },
        // plain arrows collapse selection to its bounds
        SelCase {
            label: "right_collapses_to_end",
            buf: "hello",
            caret: 1,
            sel: Some(4),
            key: press(Key::ArrowRight),
            want_buf: "hello",
            want_caret: 4,
            want_sel: None,
        },
        SelCase {
            label: "left_collapses_to_start",
            buf: "hello",
            caret: 4,
            sel: Some(1),
            key: press(Key::ArrowLeft),
            want_buf: "hello",
            want_caret: 1,
            want_sel: None,
        },
        // home/end
        SelCase {
            label: "shift_home_extends_to_zero",
            buf: "hello",
            caret: 3,
            sel: None,
            key: shift(Key::Home),
            want_buf: "hello",
            want_caret: 0,
            want_sel: Some(3),
        },
        SelCase {
            label: "shift_end_extends_to_len",
            buf: "hello",
            caret: 2,
            sel: None,
            key: shift(Key::End),
            want_buf: "hello",
            want_caret: 5,
            want_sel: Some(2),
        },
        SelCase {
            label: "home_collapses",
            buf: "hello",
            caret: 4,
            sel: Some(1),
            key: press(Key::Home),
            want_buf: "hello",
            want_caret: 0,
            want_sel: None,
        },
        // edits replace selection
        SelCase {
            label: "char_replaces_selection",
            buf: "hello",
            caret: 1,
            sel: Some(4),
            key: press(Key::Char('X')),
            want_buf: "hXo",
            want_caret: 2,
            want_sel: None,
        },
        SelCase {
            label: "backspace_deletes_selection",
            buf: "hello",
            caret: 4,
            sel: Some(1),
            key: press(Key::Backspace),
            want_buf: "ho",
            want_caret: 1,
            want_sel: None,
        },
        SelCase {
            label: "delete_deletes_selection",
            buf: "hello",
            caret: 1,
            sel: Some(4),
            key: press(Key::Delete),
            want_buf: "ho",
            want_caret: 1,
            want_sel: None,
        },
        // ctrl+a select-all
        SelCase {
            label: "ctrl_a_selects_all",
            buf: "hello",
            caret: 2,
            sel: None,
            key: ctrl(Key::Char('a')),
            want_buf: "hello",
            want_caret: 5,
            want_sel: Some(0),
        },
        SelCase {
            label: "ctrl_a_on_empty_noop",
            buf: "",
            caret: 0,
            sel: None,
            key: ctrl(Key::Char('a')),
            want_buf: "",
            want_caret: 0,
            want_sel: None,
        },
        // two-stage escape: first press collapses, leaves caret put
        SelCase {
            label: "escape_collapses_first",
            buf: "hello",
            caret: 4,
            sel: Some(1),
            key: press(Key::Escape),
            want_buf: "hello",
            want_caret: 4,
            want_sel: None,
        },
    ];
    for c in cases {
        let mut s = String::from(c.buf);
        let mut state = TextEditState {
            caret: c.caret,
            selection: c.sel,
            ..Default::default()
        };
        apply_key(&mut s, &mut state, c.key);
        assert_eq!(s, c.want_buf, "case: {} buffer", c.label);
        assert_eq!(state.caret, c.want_caret, "case: {} caret", c.label);
        assert_eq!(state.selection, c.want_sel, "case: {} selection", c.label);
    }
}

#[test]
fn escape_with_selection_does_not_blur() {
    let mut s = String::from("hello");
    let mut state = TextEditState {
        caret: 4,
        selection: Some(1),
        ..Default::default()
    };
    let blur = apply_key(&mut s, &mut state, press(Key::Escape));
    assert!(!blur, "first escape must collapse, not blur");
    assert_eq!(state.selection, None);
    let blur2 = apply_key(&mut s, &mut state, press(Key::Escape));
    assert!(blur2, "second escape (no selection) must blur");
}

#[test]
fn boundary_helpers_jump_full_codepoints() {
    let s = "héllo";
    assert_eq!(next_char_boundary(s, 0), 1);
    assert_eq!(next_char_boundary(s, 1), 3);
    assert_eq!(prev_char_boundary(s, 3), 1);
    assert_eq!(next_char_boundary(s, s.len()), s.len());
    assert_eq!(prev_char_boundary(s, 0), 0);
}

// -- Integration tests through `Ui` ---------------------------------

#[test]
fn typing_inserts_text_when_focused() {
    let mut ui = ui_with_text(SMALL);
    let mut buf = String::new();
    let id = WidgetId::from_hash("editor");

    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('h'),
        repeat: false,
    });
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('i'),
        repeat: false,
    });

    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    assert_eq!(buf, "hi");
}

#[test]
fn keystrokes_ignored_when_not_focused() {
    let mut ui = ui_with_text(SMALL);
    let mut buf = String::new();

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
    });

    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    assert_eq!(buf, "", "unfocused TextEdit must not consume keystrokes");
    assert!(ui.focused_id().is_none());
}

#[test]
fn escape_blurs_focus() {
    let mut ui = ui_with_text(SMALL);
    let mut buf = String::from("text");
    let id = WidgetId::from_hash("editor");

    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
    });
    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    assert_eq!(ui.focused_id(), None);
}

#[test]
fn caret_clamps_after_external_buffer_shrink() {
    // Host can mutate buffer between frames; if new len < cached caret,
    // `show()` must clamp at the top of the next frame instead of OOB.
    let mut ui = ui_with_text(SMALL);
    let mut buf = String::from("hello");

    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));

    buf = String::from("hi");
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
    });
    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    assert_eq!(
        buf, "hi!",
        "clamping must keep insertion at end of shrunken buffer"
    );
}

#[test]
fn text_event_inserts_at_caret_when_focused() {
    use crate::input::keyboard::TextChunk;

    let mut ui = ui_with_text(SMALL);
    let mut buf = String::new();

    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    click_at(&mut ui, Vec2::new(50.0, 20.0));

    ui.on_input(InputEvent::Text(TextChunk::new("héllo").unwrap()));
    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    assert_eq!(buf, "héllo");
}

#[test]
fn pointer_state_respects_pointer_left() {
    // Sanity: leaving the surface clears the click hit-test path so a
    // subsequent KeyDown to a focused TextEdit still works.
    let mut ui = ui_with_text(SMALL);
    let mut buf = String::new();

    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::PointerLeft);
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('z'),
        repeat: false,
    });

    run_at_acked(&mut ui, SMALL, editor_only(&mut buf));
    assert_eq!(buf, "z");
}

fn editor_and_button<'a>(buf: &'a mut String) -> impl FnMut(&mut Ui) + 'a {
    use crate::widgets::button::Button;
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("editor")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
            Button::new()
                .id_salt("plain")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }
}

#[test]
fn pressed_button_does_not_route_to_textedit_under_default_policy() {
    // Default ClearOnMiss: clicking a non-focusable Button drops focus.
    let mut ui = ui_with_text(WIDE);
    let mut buf = String::new();

    run_at_acked(&mut ui, WIDE, editor_and_button(&mut buf));
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(WidgetId::from_hash("editor")));

    run_at_acked(&mut ui, WIDE, editor_and_button(&mut buf));
    click_at(&mut ui, Vec2::new(200.0, 20.0));
    assert_eq!(
        ui.focused_id(),
        None,
        "default ClearOnMiss drops focus when clicking a non-focusable Button",
    );

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
    });
    run_at_acked(&mut ui, WIDE, editor_and_button(&mut buf));
    assert_eq!(buf, "");
}

#[test]
fn pressed_button_under_preserve_policy_keeps_focus() {
    let mut ui = ui_with_text(WIDE);
    ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
    let mut buf = String::new();

    run_at_acked(&mut ui, WIDE, editor_and_button(&mut buf));
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    run_at_acked(&mut ui, WIDE, editor_and_button(&mut buf));
    click_at(&mut ui, Vec2::new(200.0, 20.0));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('x'),
        repeat: false,
    });
    run_at_acked(&mut ui, WIDE, editor_and_button(&mut buf));
    assert_eq!(buf, "x");
}

#[test]
fn pressed_button_pointer_jitter_does_not_steal_caret() {
    // Regression: pointer movement while NOT pressed shouldn't reset caret.
    let mut ui = ui_with_text(WIDE);
    let mut buf = String::from("ab");

    run_at_acked(&mut ui, WIDE, editor_only(&mut buf));
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    run_at_acked(&mut ui, WIDE, editor_only(&mut buf));

    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 20.0)));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('!'),
        repeat: false,
    });

    run_at_acked(&mut ui, WIDE, editor_only(&mut buf));
    assert_eq!(buf, "ab!");
}

fn editor_at(buf: &mut String, padding: Option<Spacing>) -> impl FnMut(&mut Ui) + '_ {
    move |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let mut e = TextEdit::new(buf)
                .id_salt("ed")
                .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)));
            if let Some(p) = padding {
                e = e.padding(p);
            }
            e.show(ui);
        });
    }
}

#[test]
fn click_lands_caret_at_pressed_position() {
    // Mono fallback: 8 px per char @ 16 px font. With theme's default
    // 8 px left padding, x=32 → caret=3.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello world");

    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));

    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
    });
    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
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

    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, pad));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));

    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, pad));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
    });
    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, pad));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    assert_eq!(buf, "hXello world");
}

#[test]
fn two_textedits_only_one_focused_at_a_time() {
    let mut ui = ui_with_text(WIDE);
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

    run_at_acked(&mut ui, WIDE, |ui| body(ui, &mut a, &mut b));
    click_at(&mut ui, Vec2::new(50.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id_a));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('1'),
        repeat: false,
    });
    run_at_acked(&mut ui, WIDE, |ui| body(ui, &mut a, &mut b));
    assert_eq!(a, "1");
    assert_eq!(b, "");

    click_at(&mut ui, Vec2::new(250.0, 20.0));
    assert_eq!(ui.focused_id(), Some(id_b));

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('2'),
        repeat: false,
    });
    run_at_acked(&mut ui, WIDE, |ui| body(ui, &mut a, &mut b));
    assert_eq!(a, "1", "A's buffer untouched once focus moved to B");
    assert_eq!(b, "2");
}

/// `ui_at_no_cosmic` constructs a Ui without cosmic, so the mono
/// fallback drives `caret_x` (8 px/char at 16 px font) — predictable
/// widths the click-positioning tests rely on.
fn ui_at_no_cosmic(size: UVec2) -> Ui {
    use crate::layout::types::display::Display;
    let mut ui = Ui::new();
    ui.display = Display::from_physical(size, 1.0);
    ui
}

#[test]
fn each_text_widget_reads_its_own_theme_path_for_font_size() {
    use crate::TextStyle;
    use crate::forest::shapes::record::ShapeRecord;
    use crate::widgets::button::Button;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(UVec2::new(600, 200));
    ui.theme.text.font_size_px = 22.0;
    ui.theme.text_edit.normal.text = Some(TextStyle::default().with_font_size(24.0));
    let mut buf = String::from("hi");

    let mut btn_node = None;
    let mut txt_node = None;
    let mut ed_node = None;
    run_at_acked(&mut ui, UVec2::new(600, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
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
    });
    let read_fs = |node: crate::forest::tree::NodeId| -> f32 {
        shapes_of(ui.forest.tree(Layer::Main), node)
            .find_map(|s| match s {
                ShapeRecord::Text { font_size_px, .. } => Some(*font_size_px),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        read_fs(btn_node.unwrap()),
        22.0,
        "Button label falls back to theme.text"
    );
    assert_eq!(
        read_fs(txt_node.unwrap()),
        22.0,
        "Text widget reads theme.text"
    );
    assert_eq!(
        read_fs(ed_node.unwrap()),
        24.0,
        "TextEdit per-state override wins over theme.text"
    );
}

#[test]
fn theme_text_color_used_when_text_widget_does_not_override() {
    use crate::forest::shapes::record::ShapeRecord;
    use crate::primitives::color::Color;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(NARROW);
    ui.theme.text.color = Color::rgb(1.0, 0.0, 0.0);

    let mut node = None;
    run_at_acked(&mut ui, NARROW, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            node = Some(Text::new("hi").auto_id().show(ui).node);
        });
    });
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
    use crate::forest::shapes::record::ShapeRecord;
    use crate::primitives::color::Color;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(NARROW);
    ui.theme.text.color = Color::rgb(1.0, 0.0, 0.0);

    let mut node = None;
    run_at_acked(&mut ui, NARROW, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            node = Some(
                Text::new("hi")
                    .auto_id()
                    .style(TextStyle::default().with_color(Color::rgb(0.0, 1.0, 0.0)))
                    .show(ui)
                    .node,
            );
        });
    });
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
    use crate::TextStyle;
    use crate::forest::shapes::record::ShapeRecord;
    use crate::widgets::button::Button;
    use crate::widgets::text::Text;

    let mut ui = ui_at_no_cosmic(UVec2::new(600, 200));
    ui.theme.text.line_height_mult = 2.0;
    ui.theme.text_edit.normal.text = Some(TextStyle::default().with_line_height_mult(3.0));
    let mut buf = String::from("hi");

    let mut btn_node = None;
    let mut txt_node = None;
    let mut ed_node = None;
    run_at_acked(&mut ui, UVec2::new(600, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
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
    });
    let read_lh = |node: crate::forest::tree::NodeId| -> f32 {
        shapes_of(ui.forest.tree(Layer::Main), node)
            .find_map(|s| match s {
                ShapeRecord::Text { line_height_px, .. } => Some(*line_height_px),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        read_lh(btn_node.unwrap()),
        16.0 * 2.0,
        "Button label falls back to theme.text"
    );
    assert_eq!(
        read_lh(txt_node.unwrap()),
        16.0 * 2.0,
        "Text reads theme.text"
    );
    assert_eq!(
        read_lh(ed_node.unwrap()),
        16.0 * 3.0,
        "TextEdit per-state override wins over theme.text"
    );
}

#[test]
fn textedit_style_override_replaces_default_theme() {
    use crate::TextEditTheme;
    use crate::TextStyle;
    use crate::forest::shapes::record::ShapeRecord;
    use crate::widgets::theme::WidgetLook;

    for (label, mult, expected_lh) in [
        ("mult_3x_override", 3.0_f32, 48.0_f32),
        ("mult_2x_override", 2.0_f32, 32.0_f32),
    ] {
        let mut ui = ui_at_no_cosmic(NARROW);
        let mut buf = String::from("hi");
        let style = TextEditTheme {
            normal: WidgetLook {
                text: Some(TextStyle::default().with_line_height_mult(mult)),
                ..TextEditTheme::default().normal
            },
            ..TextEditTheme::default()
        };
        let mut leaf = None;
        run_at_acked(&mut ui, NARROW, |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                leaf = Some(
                    TextEdit::new(&mut buf)
                        .id_salt("ed")
                        .style(style.clone())
                        .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                        .show(ui)
                        .node,
                );
            });
        });
        let lh = shapes_of(ui.forest.tree(Layer::Main), leaf.unwrap())
            .find_map(|s| match s {
                ShapeRecord::Text { line_height_px, .. } => Some(*line_height_px),
                _ => None,
            })
            .unwrap();
        assert_eq!(lh, expected_lh, "case: {label}");
    }
}

#[test]
fn pushed_shape_carries_default_line_height_from_theme() {
    use crate::forest::shapes::record::ShapeRecord;
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hi");
    let mut leaf_node = None;
    run_at_acked(&mut ui, NARROW, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            leaf_node = Some(
                TextEdit::new(&mut buf)
                    .id_salt("ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
        });
    });
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

// -- Selection: painted highlight + drag-select ---------------------

#[test]
fn no_selection_paints_no_highlight_rect() {
    // Focused TextEdit with no selection paints exactly one
    // RoundedRect (the caret). No selection wash.
    use crate::forest::shapes::record::ShapeRecord;

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello");
    let mut leaf = None;
    let body = |ui: &mut Ui, leaf: &mut Option<crate::forest::tree::NodeId>, buf: &mut String| {
        Panel::hstack().auto_id().show(ui, |ui| {
            *leaf = Some(
                TextEdit::new(buf)
                    .id_salt("ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
        });
    };
    run_at_acked(&mut ui, NARROW, |ui| body(ui, &mut leaf, &mut buf));
    click_at(&mut ui, Vec2::new(20.0, 20.0));
    run_at_acked(&mut ui, NARROW, |ui| body(ui, &mut leaf, &mut buf));

    let rects: usize = shapes_of(ui.forest.tree(Layer::Main), leaf.unwrap())
        .filter(|s| matches!(s, ShapeRecord::RoundedRect { .. }))
        .count();
    assert_eq!(rects, 1, "only caret should paint without selection");
}

#[test]
fn shift_end_paints_selection_highlight() {
    // Programmatic Shift+End extends to len; expect a RoundedRect for
    // the selection wash, painted *before* the caret rect.
    use crate::forest::shapes::record::ShapeRecord;

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello");
    let mut leaf = None;
    let body = |ui: &mut Ui, leaf: &mut Option<crate::forest::tree::NodeId>, buf: &mut String| {
        Panel::hstack().auto_id().show(ui, |ui| {
            *leaf = Some(
                TextEdit::new(buf)
                    .id_salt("ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
        });
    };
    run_at_acked(&mut ui, NARROW, |ui| body(ui, &mut leaf, &mut buf));
    click_at(&mut ui, Vec2::new(20.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Home,
        repeat: false,
    });
    run_at_acked(&mut ui, NARROW, |ui| body(ui, &mut leaf, &mut buf));
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        shift: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    run_at_acked(&mut ui, NARROW, |ui| body(ui, &mut leaf, &mut buf));

    let rects: Vec<_> = shapes_of(ui.forest.tree(Layer::Main), leaf.unwrap())
        .filter_map(|s| match s {
            ShapeRecord::RoundedRect {
                local_rect: Some(r),
                ..
            } => Some(*r),
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 2, "expect selection wash + caret rect");
    // Selection rect is wider than the caret. Mono 8 px/char × 5 chars = 40 px.
    let widths: Vec<f32> = rects.iter().map(|r| r.size.w).collect();
    let max_w = widths.iter().copied().fold(0.0_f32, f32::max);
    assert!(
        max_w >= 40.0 - 1e-3,
        "selection wash spans buffer, got {max_w}"
    );
}

#[test]
fn drag_select_extends_selection() {
    // Press at offset 1, drag to offset 4 → selection covers [1..4].
    // Mono fallback: 8 px/char, theme pad-left = 8 px → byte offset N
    // sits at x = 8 + 8N.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello");

    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    // Mouse-down at offset 1 (x = 16).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(16.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    // Drag to offset 4 (x = 40) — still pressed.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 20.0)));
    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    // Type 'X' — replaces the selected range.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
    });
    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    assert_eq!(
        buf, "hXo",
        "drag-selected [1..4] then 'X' typed: 'h' + 'X' + 'o'"
    );
}

#[test]
fn click_without_drag_clears_prior_selection() {
    // Programmatic Ctrl+A select-all, then a press elsewhere should
    // collapse the selection (anchor latched on the press, no drag).
    // Uses press+frame+release so the rising edge actually fires.
    use crate::support::testing::{press_at, release_left};
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello");

    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    click_at(&mut ui, Vec2::new(20.0, 20.0));
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('a'),
        repeat: false,
    });
    ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE));
    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));

    // Now press at offset 2 (x = 8 + 16 = 24), let a frame run, release.
    press_at(&mut ui, Vec2::new(24.0, 20.0));
    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    release_left(&mut ui);

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('Z'),
        repeat: false,
    });
    run_at_acked(&mut ui, NARROW, editor_at(&mut buf, None));
    assert_eq!(
        buf, "heZllo",
        "click clears selection; 'Z' inserts at caret 2"
    );
}

#[test]
fn line_height_override_changes_caret_rect_height() {
    // Pin: caret rect height tracks the leading carried on the
    // theme's `text` style.
    use crate::TextEditTheme;
    use crate::TextStyle;
    use crate::forest::shapes::record::ShapeRecord;
    use crate::widgets::theme::WidgetLook;

    fn caret_height(style: Option<TextEditTheme>) -> f32 {
        let mut ui = ui_at_no_cosmic(NARROW);
        let mut buf = String::new();
        let mut leaf = None;
        let body = |ui: &mut Ui,
                    leaf: &mut Option<crate::forest::tree::NodeId>,
                    buf: &mut String,
                    style: &Option<TextEditTheme>| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let mut e = TextEdit::new(buf)
                    .id_salt("ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)));
                if let Some(s) = style.clone() {
                    e = e.style(s);
                }
                *leaf = Some(e.show(ui).node);
            });
        };
        run_at_acked(&mut ui, NARROW, |ui| body(ui, &mut leaf, &mut buf, &style));
        click_at(&mut ui, Vec2::new(20.0, 20.0));
        run_at_acked(&mut ui, NARROW, |ui| body(ui, &mut leaf, &mut buf, &style));
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
        run_at(&mut *ui, SMALL, |ui| body(ui, buf));
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
        click_at(ui, Vec2::new(body_rect.min.x + 20.0, row_y));
        run_at(&mut *ui, SMALL, |ui| body(ui, buf));
    }

    // Seed: buffer with text, select "ell" (caret=4, anchor=1).
    let mut ui = ui_at_no_cosmic(SMALL);
    let mut buf = String::from("hello");
    run_at_acked(&mut ui, SMALL, |ui| body(ui, &mut buf));
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
    let st = *ui.state_mut::<TextEditState>(editor_id());
    assert_eq!(st.caret, 1);
    assert_eq!(st.selection, None);

    // Paste at caret → "h" + "ell" + "o" = "hello".
    open_menu_and_record(&mut ui, &mut buf);
    click_menu_row(&mut ui, &mut buf, 2); // row 2 == Paste
    assert_eq!(buf, "hello", "paste inserts clipboard at caret");
    let st = *ui.state_mut::<TextEditState>(editor_id());
    assert_eq!(st.caret, 4, "caret advances past pasted text");

    // Clear → buffer wiped, caret reset. Row 3 is the separator;
    // row 4 is Clear in render order.
    open_menu_and_record(&mut ui, &mut buf);
    click_menu_row(&mut ui, &mut buf, 4);
    assert_eq!(buf, "");
    let st = *ui.state_mut::<TextEditState>(editor_id());
    assert_eq!(st.caret, 0);
}

/// Platform clipboard shortcuts — Cmd/Ctrl + C / X / V mutate the
/// buffer and the clipboard exactly like the corresponding menu
/// items. Both `ctrl` and `meta` accepted on every platform (single
/// table-driven sweep over the two modifier flags + 3 actions).
#[test]
fn clipboard_shortcuts_apply_keypresses() {
    let _cb_guard = crate::clipboard::test_serialize_guard();

    /// Build a `KeyPress` with one of `ctrl` / `meta` set.
    fn shortcut(c: char, meta_not_ctrl: bool) -> KeyPress {
        let mut mods = Modifiers::NONE;
        if meta_not_ctrl {
            mods.meta = true;
        } else {
            mods.ctrl = true;
        }
        KeyPress {
            key: Key::Char(c),
            mods,
            repeat: false,
        }
    }

    // Copy then cut then paste, each through both ctrl and meta. Same
    // state machine as the menu test, just driven via key events.
    for &use_meta in &[false, true] {
        crate::clipboard::set("");
        let mut text = String::from("hello");
        let mut state = TextEditState {
            caret: 4,
            selection: Some(1),
            ..TextEditState::default()
        };

        // Copy: clipboard ← "ell", buffer unchanged.
        super::apply_key(&mut text, &mut state, shortcut('c', use_meta));
        assert_eq!(text, "hello");
        assert_eq!(crate::clipboard::get(), "ell");

        // Cut: clipboard keeps "ell", buffer drops it, caret collapses.
        super::apply_key(&mut text, &mut state, shortcut('x', use_meta));
        assert_eq!(text, "ho");
        assert_eq!(crate::clipboard::get(), "ell");
        assert_eq!(state.caret, 1);
        assert_eq!(state.selection, None);

        // Paste: insert clipboard at caret → "hello".
        super::apply_key(&mut text, &mut state, shortcut('v', use_meta));
        assert_eq!(text, "hello");
        assert_eq!(state.caret, 4);
    }
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
    super::apply_key(
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
    run_at_acked(&mut ui, SMALL, |ui| body(ui, &mut buf));
    assert!(!ContextMenu::is_open(&ui, editor_id));

    secondary_click_at(&mut ui, Vec2::new(40.0, 20.0));
    run_at(&mut ui, SMALL, |ui| body(ui, &mut buf));
    assert!(
        ContextMenu::is_open(&ui, editor_id),
        "secondary click on TextEdit opens its default menu",
    );
}
