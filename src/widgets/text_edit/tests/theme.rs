use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::rect::RectKind;
use crate::widgets::text_edit::tests::*;
use crate::widgets::theme::text_style::{LINE_HEIGHT_MULT, TextStyle, TextStyleOverrides};

#[test]
fn each_text_widget_reads_its_own_theme_path_for_font_size() {
    use crate::shape::record::ShapeRecord;
    use crate::widgets::button::Button;
    use crate::widgets::text::Text;

    let mut h = UiHarness::new(UVec2::new(600, 200));
    h.ui.theme_mut().text.font_size = 22.0;
    h.ui.theme_mut().text_edit.looks.normal.text = TextStyleOverrides::NONE.with_font_size(24.0);
    let mut buf = String::from("hi");

    let [btn_node, txt_node, ed_node] = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .show(ui, |ui| {
                [
                    Button::new()
                        .id(WidgetId::from_hash("btn"))
                        .label("hi")
                        .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                        .show(ui)
                        .node(),
                    Text::new("hi").auto_id().show(ui).node(),
                    TextEdit::new(&mut buf)
                        .id(WidgetId::from_hash("ed"))
                        .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                        .show(ui)
                        .response
                        .node(),
                ]
            })
            .inner
    });
    let read_fs = |node: NodeId| -> f32 {
        painted_shapes(&h.ui, node)
            .find_map(|s| match s {
                ShapeRecord::Text { font, .. } => Some(font.size),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        read_fs(btn_node),
        22.0,
        "Button label falls back to theme.text"
    );
    assert_eq!(read_fs(txt_node), 22.0, "Text widget reads theme.text");
    assert_eq!(
        read_fs(ed_node),
        24.0,
        "TextEdit per-state override wins over theme.text"
    );
}

#[test]
fn theme_text_color_used_when_text_widget_does_not_override() {
    use crate::primitives::paint::color::RgbaF32;
    use crate::shape::record::ShapeRecord;
    use crate::widgets::text::Text;

    let mut h = UiHarness::new(NARROW);
    h.ui.theme_mut().text.color = RgbaF32::srgb(1.0, 0.0, 0.0);

    let node = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| Text::new("hi").auto_id().show(ui).node())
            .inner
    });
    let color = painted_shapes(&h.ui, node)
        .find_map(|s| match s {
            ShapeRecord::Text { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(RgbaF32::from(color), RgbaF32::srgb(1.0, 0.0, 0.0));
}

#[test]
fn text_widget_color_override_wins_over_theme() {
    use crate::primitives::paint::color::RgbaF32;
    use crate::shape::record::ShapeRecord;
    use crate::widgets::text::Text;

    let mut h = UiHarness::new(NARROW);
    h.ui.theme_mut().text.color = RgbaF32::srgb(1.0, 0.0, 0.0);

    let node = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                Text::new("hi")
                    .auto_id()
                    .style(&TextStyle::default().with_color(RgbaF32::srgb(0.0, 1.0, 0.0)))
                    .show(ui)
                    .node()
            })
            .inner
    });
    let color = painted_shapes(&h.ui, node)
        .find_map(|s| match s {
            ShapeRecord::Text { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(RgbaF32::from(color), RgbaF32::srgb(0.0, 1.0, 0.0));
}

#[test]
fn each_text_widget_reads_its_own_theme_path_for_line_height() {
    use crate::shape::record::ShapeRecord;
    use crate::widgets::button::Button;
    use crate::widgets::text::Text;

    let mut h = UiHarness::new(UVec2::new(600, 200));
    h.ui.theme_mut().text.line_height_factor = 2.0;
    h.ui.theme_mut().text_edit.looks.normal.text =
        TextStyleOverrides::NONE.with_line_height_factor(3.0);
    let mut buf = String::from("hi");

    let [btn_node, txt_node, ed_node] = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .show(ui, |ui| {
                [
                    Button::new()
                        .id(WidgetId::from_hash("btn"))
                        .label("hi")
                        .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                        .show(ui)
                        .node(),
                    Text::new("hi").auto_id().show(ui).node(),
                    TextEdit::new(&mut buf)
                        .id(WidgetId::from_hash("ed"))
                        .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                        .show(ui)
                        .response
                        .node(),
                ]
            })
            .inner
    });
    let read_lh = |node: NodeId| -> f32 {
        painted_shapes(&h.ui, node)
            .find_map(|s| match s {
                ShapeRecord::Text { font, .. } => Some(font.line_height),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        read_lh(btn_node),
        16.0 * 2.0,
        "Button label falls back to theme.text"
    );
    assert_eq!(read_lh(txt_node), 16.0 * 2.0, "Text reads theme.text");
    assert_eq!(
        read_lh(ed_node),
        16.0 * 3.0,
        "TextEdit per-state override wins over theme.text"
    );
}

#[test]
fn invalid_runtime_metrics_record_no_text_or_shaping_state() {
    use crate::primitives::math::domain::EPS;
    use crate::shape::record::ShapeRecord;
    use crate::widgets::text::Text;
    use crate::widgets::text_edit::TextEditState;

    let cases = [
        ("zero font", 0.0, 1.2),
        ("negative font", -1.0, 1.2),
        ("sub-epsilon font", EPS * 0.5, 1.2),
        ("epsilon font", EPS, 1.2),
        ("NaN font", f32::NAN, 1.2),
        ("infinite font", f32::INFINITY, 1.2),
        ("zero line height", 16.0, 0.0),
        ("negative line height", 16.0, -1.0),
        ("sub-epsilon line height", 16.0, EPS / 32.0),
        ("epsilon line height", 16.0, EPS / 16.0),
        ("NaN line height", 16.0, f32::NAN),
        ("infinite line height", 16.0, f32::INFINITY),
    ];

    for (label, font_size, line_height_factor) in cases {
        let style = TextStyle {
            font_size,
            line_height_factor,
            ..TextStyle::default()
        };
        let editor_id = WidgetId::from_hash("invalid editor");
        let mut h = UiHarness::new(UVec2::new(600, 200));
        let mut buf = String::from("editable");

        // One renderable frame first, so the unrenderable one below has
        // retained state it could lose.
        h.frame(|ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                TextEdit::new(&mut buf)
                    .id(editor_id)
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui);
            });
        });
        h.ui.with_state::<TextEditState, _>(editor_id, |_, st| {
            st.edit.caret = 4;
            st.edit.selection = Some(1);
        });

        h.ui.theme_mut().text_edit.looks.normal.text = TextStyleOverrides::NONE
            .with_font_size(font_size)
            .with_line_height_factor(line_height_factor);
        let calls = h.ui.shaper().measure_calls();

        let nodes = h.frame_value(|ui| {
            Panel::vstack()
                .auto_id()
                .show(ui, |ui| {
                    [
                        Text::new("label").style(&style).show(ui).node(),
                        TextEdit::new(&mut buf)
                            .id(editor_id)
                            .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                            .show(ui)
                            .response
                            .node(),
                    ]
                })
                .inner
        });

        for node in nodes {
            assert!(
                painted_shapes(&h.ui, node).all(|shape| !matches!(shape, ShapeRecord::Text { .. })),
                "{label}: invalid text entered the recorded shape stream",
            );
        }
        assert_eq!(
            h.ui.shaper().measure_calls(),
            calls,
            "{label}: invalid text reached the shaper",
        );
        // `show` moves the state row out for the whole pass and back at
        // the end. Bailing here is the pass's *only* early return, so it
        // is the one place a write-back on some-but-not-all paths would
        // hand the row back as `Default` — silently resetting the caret
        // and dropping the undo stack the moment a theme made the text
        // unrenderable.
        let st = h.state::<TextEditState>(editor_id);
        assert_eq!(st.edit.caret, 4, "{label}: caret lost on the early return");
        assert_eq!(
            st.edit.selection,
            Some(1),
            "{label}: selection lost on the early return",
        );
    }
}

#[test]
fn textedit_style_override_replaces_default_theme() {
    use crate::TextEditTheme;
    use crate::shape::record::ShapeRecord;
    use crate::widget_core::widget_look::WidgetLook;
    use crate::widget_core::widget_look::stateful_look::StatefulLook;

    for (label, factor, expected_lh) in [
        ("mult_3x_override", 3.0_f32, 48.0_f32),
        ("mult_2x_override", 2.0_f32, 32.0_f32),
    ] {
        let mut h = UiHarness::new(NARROW);
        let mut buf = String::from("hi");
        let style = TextEditTheme {
            looks: StatefulLook {
                normal: WidgetLook {
                    text: TextStyleOverrides::NONE.with_line_height_factor(factor),
                    ..TextEditTheme::default().looks.normal
                },
                ..TextEditTheme::default().looks
            },
            ..TextEditTheme::default()
        };
        let leaf = h.frame_value(|ui| {
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    TextEdit::new(&mut buf)
                        .id(WidgetId::from_hash("ed"))
                        .style(&style)
                        .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                        .show(ui)
                        .response
                        .node()
                })
                .inner
        });
        let lh = painted_shapes(&h.ui, leaf)
            .find_map(|s| match s {
                ShapeRecord::Text { font, .. } => Some(font.line_height),
                _ => None,
            })
            .unwrap();
        assert_eq!(lh, expected_lh, "case: {label}");
    }
}

#[test]
fn pushed_shape_carries_default_line_height_from_theme() {
    use crate::shape::record::ShapeRecord;
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("hi");
    let leaf_node = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                TextEdit::new(&mut buf)
                    .id(WidgetId::from_hash("ed"))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .response
                    .node()
            })
            .inner
    });
    let text_shape = painted_shapes(&h.ui, leaf_node).find_map(|s| match s {
        ShapeRecord::Text { font, .. } => Some((font.size, font.line_height)),
        _ => None,
    });
    let (fs, lh) = text_shape.expect("TextEdit pushes a ShapeRecord::Text for non-empty buffer");
    assert_eq!(fs, 16.0);
    // 16 × 1.2 = 19.2, on the shaper's 1/64-px grid: 1228.8 64ths round
    // to 1229.
    assert_eq!(
        lh,
        (16.0 * LINE_HEIGHT_MULT * 64.0).round() / 64.0,
        "default line_height is font_size * LINE_HEIGHT_MULT on the 1/64 grid, got {lh}"
    );
    assert_eq!(lh, 19.203125);
}

#[test]
fn no_selection_paints_no_highlight_rect() {
    // Focused TextEdit with no selection paints exactly one
    // rounded rect (the caret). No selection wash.
    use crate::shape::record::ShapeRecord;

    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("hello");
    let body = |ui: &mut Ui, buf: &mut String| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                TextEdit::new(buf)
                    .id(WidgetId::from_hash("ed"))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .response
                    .node()
            })
            .inner
    };
    h.frame(|ui| {
        body(ui, &mut buf);
    });
    h.click_at(Vec2::new(20.0, 20.0));
    let leaf = h.frame_value(|ui| body(ui, &mut buf));

    let rects: usize = painted_shapes(&h.ui, leaf)
        .filter(|s| {
            matches!(
                s,
                ShapeRecord::Quad(QuadShape::Rect {
                    kind: RectKind::Rounded,
                    ..
                })
            )
        })
        .count();
    assert_eq!(rects, 1, "only caret should paint without selection");
}

#[test]
fn shift_end_paints_selection_highlight() {
    // Programmatic Shift+End extends to len; expect a rounded rect for
    // the selection wash, painted *before* the caret rect.
    use crate::shape::record::ShapeRecord;

    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("hello");
    let body = |ui: &mut Ui, buf: &mut String| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                TextEdit::new(buf)
                    .id(WidgetId::from_hash("ed"))
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui)
                    .response
                    .node()
            })
            .inner
    };
    h.frame(|ui| {
        body(ui, &mut buf);
    });
    h.click_at(Vec2::new(20.0, 20.0));
    h.key(Key::Home);
    h.frame(|ui| {
        body(ui, &mut buf);
    });
    h.set_modifiers(Modifiers::SHIFT);
    h.key(Key::End);
    let leaf = h.frame_value(|ui| body(ui, &mut buf));

    let rects: Vec<_> = painted_shapes(&h.ui, leaf)
        .filter_map(|s| match s {
            ShapeRecord::Quad(QuadShape::Rect {
                kind: RectKind::Rounded,
                local_rect: Some(r),
                ..
            }) => Some(*r),
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
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("hello");

    h.frame(editor_at(&mut buf, None));
    // Mouse-down at offset 1 (x = 16).
    h.press_at(Vec2::new(16.0, 20.0));
    h.frame(editor_at(&mut buf, None));
    // Drag to offset 4 (x = 40) — still pressed.
    h.drag_to(Vec2::new(40.0, 20.0));
    h.frame(editor_at(&mut buf, None));
    h.release();

    h.key(Key::Char('X'));
    h.frame(editor_at(&mut buf, None));
    assert_eq!(
        buf, "hXo",
        "drag-selected [1..4] then 'X' typed: 'h' + 'X' + 'o'"
    );
}

#[test]
fn click_without_drag_clears_prior_selection() {
    // Programmatic Ctrl+A select-all, then a press elsewhere should
    // collapse the selection (anchor latched on the press, no drag).
    // Uses press+frame+release so the rising edge actually fires. The
    // focusing click sits > DOUBLE_CLICK_RADIUS from the later press —
    // the input layer counts *every* press toward a multi-press run
    // (frames don't have to observe them), so a same-spot follow-up
    // would legitimately read as a double-click word-select.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("hello");

    h.frame(editor_at(&mut buf, None));
    h.click_at(Vec2::new(60.0, 20.0));
    h.set_modifiers(Modifiers::CTRL);
    h.key(Key::Char('a'));
    h.set_modifiers(Modifiers::NONE);
    h.frame(editor_at(&mut buf, None));

    // Now press at offset 2 (x = 8 + 16 = 24), let a frame run, release.
    h.press_at(Vec2::new(24.0, 20.0));
    h.frame(editor_at(&mut buf, None));
    h.release();

    h.key(Key::Char('Z'));
    h.frame(editor_at(&mut buf, None));
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
    use crate::shape::record::ShapeRecord;
    use crate::widget_core::widget_look::WidgetLook;
    use crate::widget_core::widget_look::stateful_look::StatefulLook;

    fn caret_height(style: Option<&TextEditTheme>) -> f32 {
        let mut h = UiHarness::new(NARROW);
        let mut buf = String::new();
        let body = |ui: &mut Ui, buf: &mut String, style: Option<&TextEditTheme>| {
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    let mut e = TextEdit::new(buf)
                        .id(WidgetId::from_hash("ed"))
                        .size((Sizing::fixed(180.0), Sizing::fixed(40.0)));
                    if let Some(s) = style {
                        e = e.style(s);
                    }
                    e.show(ui).response.node()
                })
                .inner
        };
        h.frame(|ui| {
            body(ui, &mut buf, style);
        });
        h.click_at(Vec2::new(20.0, 20.0));
        let leaf = h.frame_value(|ui| body(ui, &mut buf, style));
        painted_shapes(&h.ui, leaf)
            .find_map(|s| match s {
                ShapeRecord::Quad(QuadShape::Rect {
                    kind: RectKind::Rounded,
                    local_rect: Some(rect),
                    ..
                }) => Some(rect.size.h),
                _ => None,
            })
            .expect("focused TextEdit pushes a caret Overlay")
    }

    let default = caret_height(None);
    let doubled = caret_height(Some(&TextEditTheme {
        looks: StatefulLook {
            active: WidgetLook {
                text: TextStyleOverrides::NONE.with_line_height_factor(2.0),
                ..TextEditTheme::default().looks.active
            },
            ..TextEditTheme::default().looks
        },
        ..TextEditTheme::default()
    }));
    let canonical_default: f32 = (16.0 * LINE_HEIGHT_MULT * 64.0).round() / 64.0;
    assert_eq!(
        default, canonical_default,
        "default caret height = canonical font_size * LINE_HEIGHT_MULT, got {default}"
    );
    assert_eq!(
        doubled, 32.0,
        "2.0 multiplier yields 32 px caret, got {doubled}"
    );
}
