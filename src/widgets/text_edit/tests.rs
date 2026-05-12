use super::{apply_key, next_char_boundary, prev_char_boundary};
use crate::Spacing;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::sizing::Sizing;
use crate::support::testing::{click_at, run_at_acked, shapes_of, ui_with_text};
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

#[test]
fn pressed_button_event_left_click_release_one_frame() {
    let _ = PointerButton::Left;
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
    use crate::forest::shapes::ShapeRecord;
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
    use crate::forest::shapes::ShapeRecord;
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
    use crate::forest::shapes::ShapeRecord;
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
    use crate::forest::shapes::ShapeRecord;
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
    use crate::forest::shapes::ShapeRecord;
    use crate::widgets::theme::WidgetLook;

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hi");
    let style = TextEditTheme {
        normal: WidgetLook {
            text: Some(TextStyle::default().with_line_height_mult(3.0)),
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
    assert_eq!(lh, 48.0, "16 px font × 3.0 leading override = 48");
}

#[test]
fn pushed_shape_carries_default_line_height_from_theme() {
    use crate::forest::shapes::ShapeRecord;
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

#[test]
fn pushed_shape_uses_style_overridden_line_height() {
    use crate::TextEditTheme;
    use crate::TextStyle;
    use crate::forest::shapes::ShapeRecord;
    use crate::widgets::theme::WidgetLook;
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hi");
    let style = TextEditTheme {
        normal: WidgetLook {
            text: Some(TextStyle::default().with_line_height_mult(2.0)),
            ..TextEditTheme::default().normal
        },
        ..TextEditTheme::default()
    };
    let mut leaf_node = None;
    run_at_acked(&mut ui, NARROW, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            leaf_node = Some(
                TextEdit::new(&mut buf)
                    .id_salt("ed")
                    .style(style.clone())
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
        });
    });
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
    // theme's `text` style.
    use crate::TextEditTheme;
    use crate::TextStyle;
    use crate::forest::shapes::ShapeRecord;
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
