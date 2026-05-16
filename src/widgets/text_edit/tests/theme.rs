use super::*;
use crate::widgets::test_support::ResponseNodeExt;

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
    ui.run_at_acked(UVec2::new(600, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            btn_node = Some(
                Button::new()
                    .id_salt("btn")
                    .label("hi")
                    .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node(ui),
            );
            txt_node = Some(Text::new("hi").auto_id().show(ui).node(ui));
            ed_node = Some(
                TextEdit::new(&mut buf)
                    .id_salt("ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node(ui),
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
    ui.run_at_acked(NARROW, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            node = Some(Text::new("hi").auto_id().show(ui).node(ui));
        });
    });
    let color = shapes_of(ui.forest.tree(Layer::Main), node.unwrap())
        .find_map(|s| match s {
            ShapeRecord::Text { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(Color::from(color), Color::rgb(1.0, 0.0, 0.0));
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
    ui.run_at_acked(NARROW, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            node = Some(
                Text::new("hi")
                    .auto_id()
                    .style(TextStyle::default().with_color(Color::rgb(0.0, 1.0, 0.0)))
                    .show(ui)
                    .node(ui),
            );
        });
    });
    let color = shapes_of(ui.forest.tree(Layer::Main), node.unwrap())
        .find_map(|s| match s {
            ShapeRecord::Text { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(Color::from(color), Color::rgb(0.0, 1.0, 0.0));
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
    ui.run_at_acked(UVec2::new(600, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            btn_node = Some(
                Button::new()
                    .id_salt("btn")
                    .label("hi")
                    .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node(ui),
            );
            txt_node = Some(Text::new("hi").auto_id().show(ui).node(ui));
            ed_node = Some(
                TextEdit::new(&mut buf)
                    .id_salt("ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node(ui),
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
        ui.run_at_acked(NARROW, |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                leaf = Some(
                    TextEdit::new(&mut buf)
                        .id_salt("ed")
                        .style(style.clone())
                        .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                        .show(ui)
                        .node(ui),
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
    ui.run_at_acked(NARROW, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            leaf_node = Some(
                TextEdit::new(&mut buf)
                    .id_salt("ed")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node(ui),
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
                    .node(ui),
            );
        });
    };
    ui.run_at_acked(NARROW, |ui| body(ui, &mut leaf, &mut buf));
    ui.click_at(Vec2::new(20.0, 20.0));
    ui.run_at_acked(NARROW, |ui| body(ui, &mut leaf, &mut buf));

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
                    .node(ui),
            );
        });
    };
    ui.run_at_acked(NARROW, |ui| body(ui, &mut leaf, &mut buf));
    ui.click_at(Vec2::new(20.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Home,
        repeat: false,
    });
    ui.run_at_acked(NARROW, |ui| body(ui, &mut leaf, &mut buf));
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        shift: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    ui.run_at_acked(NARROW, |ui| body(ui, &mut leaf, &mut buf));

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

    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
    // Mouse-down at offset 1 (x = 16).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(16.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
    // Drag to offset 4 (x = 40) — still pressed.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 20.0)));
    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    // Type 'X' — replaces the selected range.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('X'),
        repeat: false,
    });
    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
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
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello");

    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
    ui.click_at(Vec2::new(20.0, 20.0));
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('a'),
        repeat: false,
    });
    ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE));
    ui.run_at_acked(NARROW, editor_at(&mut buf, None));

    // Now press at offset 2 (x = 8 + 16 = 24), let a frame run, release.
    ui.press_at(Vec2::new(24.0, 20.0));
    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
    ui.release_left();

    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('Z'),
        repeat: false,
    });
    ui.run_at_acked(NARROW, editor_at(&mut buf, None));
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
                *leaf = Some(e.show(ui).node(ui));
            });
        };
        ui.run_at_acked(NARROW, |ui| body(ui, &mut leaf, &mut buf, &style));
        ui.click_at(Vec2::new(20.0, 20.0));
        ui.run_at_acked(NARROW, |ui| body(ui, &mut leaf, &mut buf, &style));
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
