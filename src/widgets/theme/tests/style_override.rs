//! What every widget's `style` setter is: the `Option`-taking setter, and the one
//! naming of a widget's theme slot that resolves it.

use crate::primitives::color::RgbaF32;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::harness::UiHarness;
use crate::widgets::button::Button;
use crate::widgets::configure::Configure;
use crate::widgets::text::Text;
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
