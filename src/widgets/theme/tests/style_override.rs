//! What every widget's `style` setter is: the `Option`-taking setter, and the one
//! naming of a widget's theme slot that resolves it.

use crate::primitives::color::RgbaF16;
use crate::primitives::color::RgbaF32;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::shapes::record::ShapeRecord;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::ui::harness::UiHarness;
use crate::widgets::button::Button;
use crate::widgets::configure::Configure;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::Theme;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::theme_slot::SlotDefaults;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(400, 200);

/// A styled instance and an unstyled one, side by side, with the override
/// passed *as data*: `Some(&theme)` and `None` both go through the same
/// `.style(…)` call. Expressing "styled or default" without branching around
/// the widget is the point of the `impl Into<Option<&T>>` signature.
///
/// Pinned on padding rather than colour because padding lands in layout,
/// where the test can read it back off the arranged rect.
#[test]
fn style_takes_an_option_and_none_falls_back_to_the_slot() {
    let theme = Theme {
        button: ButtonTheme {
            defaults: SlotDefaults {
                padding: Spacing::all(4.0),
                ..ButtonTheme::default().defaults
            },
            ..ButtonTheme::default()
        },
        ..Theme::default()
    };
    let wide = ButtonTheme {
        defaults: SlotDefaults {
            padding: Spacing::all(20.0),
            ..ButtonTheme::default().defaults
        },
        ..ButtonTheme::default()
    };

    let styled_id = WidgetId::from_hash("style-override-styled");
    let plain_id = WidgetId::from_hash("style-override-plain");

    let mut h = UiHarness::new(SURFACE);
    h.ui.set_theme(theme);
    h.frame(|ui| {
        for (id, style) in [(styled_id, Some(&wide)), (plain_id, None)] {
            Button::new().id(id).label("x").style(style).show(ui);
        }
    });

    let widths = |id: WidgetId| {
        let node = h.node_for_widget_id(id);
        h.ui.arranged_rect(Layer::Main, node).size.w
    };
    // 20 px of padding a side against 4 px a side: the styled button is
    // exactly 2 × (20 − 4) = 32 px wider around the same one-glyph label.
    assert_eq!(
        widths(styled_id) - widths(plain_id),
        32.0,
        "`Some(&theme)` overrides padding, `None` inherits `Theme::button`",
    );
}

/// `Text`'s slot is `Theme::text`, not a per-widget bundle — the same
/// `style` shape reaching a different kind of slot.
#[test]
fn text_style_none_inherits_the_ambient_text_style() {
    let theme = Theme {
        text: TextStyle::default()
            .with_font_size(31.0)
            .with_color(RgbaF32::srgb(1.0, 0.0, 0.0)),
        ..Theme::default()
    };
    let big = TextStyle::default().with_font_size(62.0);

    let plain_id = WidgetId::from_hash("style-text-plain");
    let styled_id = WidgetId::from_hash("style-text-styled");

    let mut h = UiHarness::new(SURFACE);
    h.ui.set_theme(theme);
    h.frame(|ui| {
        Text::new("m").id(plain_id).style(None).show(ui);
        Text::new("m").id(styled_id).style(Some(&big)).show(ui);
    });

    let height = |id: WidgetId| {
        let node = h.node_for_widget_id(id);
        h.ui.arranged_rect(Layer::Main, node).size.h
    };
    // Twice the font size, twice the single line's height.
    assert_eq!(height(styled_id), height(plain_id) * 2.0);
}

/// A per-axis setter reaches the shaper, and outranks the bundle beneath it.
///
/// [`Text`] and [`TextEdit`](crate::TextEdit) offer the same chain, so both
/// are pinned. The editor is the case that could have missed the fold: it
/// resolves its face through an animated look rather than a plain style.
#[test]
fn per_axis_setters_outrank_the_style_bundle() {
    const AMBIENT: f32 = 20.0;
    const BUNDLE: f32 = 40.0;
    const PER_AXIS: f32 = 32.0;
    let ambient_color = RgbaF32::srgb(1.0, 0.0, 0.0);
    let axis_color = RgbaF32::srgb(0.0, 1.0, 0.0);
    let theme = Theme {
        text: TextStyle::default()
            .with_font_size(AMBIENT)
            .with_color(ambient_color),
        ..Theme::default()
    };
    let bundle = TextStyle::default().with_font_size(BUNDLE);

    let plain = WidgetId::from_hash("axis-plain");
    let axes = WidgetId::from_hash("axis-set");
    let over_bundle = WidgetId::from_hash("axis-over-bundle");
    let edit_plain = WidgetId::from_hash("axis-edit-plain");
    let edit_axes = WidgetId::from_hash("axis-edit-set");

    let mut h = UiHarness::new(SURFACE);
    h.ui.set_theme(theme);
    let mut plain_buf = String::from("m");
    let mut axes_buf = String::from("m");
    h.frame(|ui| {
        Text::new("m").id(plain).show(ui);
        Text::new("m")
            .id(axes)
            .font_size(PER_AXIS)
            .color(axis_color)
            .weight(FontWeight::BOLD)
            .slant(FontSlant::Italic)
            .line_height(2.0)
            .show(ui);
        Text::new("m")
            .id(over_bundle)
            .style(&bundle)
            .font_size(PER_AXIS)
            .show(ui);
        TextEdit::new(&mut plain_buf).id(edit_plain).show(ui);
        TextEdit::new(&mut axes_buf)
            .id(edit_axes)
            .font_size(PER_AXIS)
            .color(axis_color)
            .show(ui);
    });

    // Unset axes keep the ambient style's value; a set axis replaces it,
    // whatever the bundle under it said.
    let face = face_of(&h, plain);
    assert_eq!(face.font.size_px, AMBIENT);
    assert_eq!(face.font.weight, FontWeight::REGULAR);
    assert_eq!(face.color, ambient_color.into());

    let face = face_of(&h, axes);
    assert_eq!(face.font.size_px, PER_AXIS);
    // `line_height(2.0)` is a multiple of the *overridden* size.
    assert_eq!(face.font.line_height_px, PER_AXIS * 2.0);
    assert_eq!(face.font.weight, FontWeight::BOLD);
    assert_eq!(face.font.slant, FontSlant::Italic);
    assert_eq!(face.color, axis_color.into());

    assert_eq!(
        face_of(&h, over_bundle).font.size_px,
        PER_AXIS,
        "a per-axis setter wins over the bundle `style` supplied",
    );

    let plain_face = face_of(&h, edit_plain.with("text-block"));
    assert_eq!(plain_face.font.size_px, AMBIENT);
    let axes_face = face_of(&h, edit_axes.with("text-block"));
    assert_eq!(axes_face.font.size_px, PER_AXIS);
    assert_eq!(axes_face.color, axis_color.into());
}

/// What one `Shape::Text` was recorded with.
struct RecordedFace {
    font: GlyphFont,
    color: RgbaF16,
}

fn face_of(h: &UiHarness, id: WidgetId) -> RecordedFace {
    let node = h.node_for_widget_id(id);
    h.ui.tree(Layer::Main)
        .shapes_of(node)
        .find_map(|s| match s {
            ShapeRecord::Text { font, color, .. } => Some(RecordedFace {
                font: *font,
                color: *color,
            }),
            _ => None,
        })
        .expect("the widget records one text shape")
}
