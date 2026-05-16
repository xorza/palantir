pub(crate) mod button;
pub(crate) mod context_menu;
pub(crate) mod palette;
pub(crate) mod scrollbar;
pub(crate) mod text_edit;
pub(crate) mod text_style;
pub(crate) mod toggle;
pub(crate) mod tooltip;
pub(crate) mod widget_look;

use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::context_menu::ContextMenuTheme;
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::theme::tooltip::TooltipTheme;

/// Global theme. Aggregates per-widget themes. Widgets opt in by reading
/// from `Ui::theme`.
///
/// The framework does not auto-dim disabled subtrees — that's an
/// app/theme concern. Widgets that want disabled-state visuals read the
/// disabled flag themselves and pick their own colors at recording
/// time.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Theme {
    pub button: ButtonTheme,
    pub checkbox: ToggleTheme,
    pub radio: ToggleTheme,
    pub scrollbar: ScrollbarTheme,
    pub text_edit: TextEditTheme,
    pub context_menu: ContextMenuTheme,
    pub tooltip: TooltipTheme,
    pub text: TextStyle,
    /// Window/swapchain clear color. Hosts pass to `WgpuBackend::submit`.
    pub window_clear: Color,
    /// Default chrome paint for container widgets (`Panel`, `Grid`,
    /// `Popup`) that didn't call [`crate::Configure::background`].
    /// `None` leaves containers unpainted by default. Setting
    /// `Some(...)` lights up every unstyled container at once — useful
    /// for prototyping or shipping a design-system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_background: Option<Background>,
    /// Default clip mode for container widgets that didn't call
    /// `Configure::clip_rect` / `Configure::clip_rounded`. Pairs with
    /// [`Self::panel_background`]; the chrome's `radius` supplies the
    /// rounded-clip mask geometry.
    #[serde(default, skip_serializing_if = "is_clip_none")]
    pub panel_clip: ClipMode,
}

#[inline]
fn is_clip_none(c: &ClipMode) -> bool {
    matches!(c, ClipMode::None)
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            button: ButtonTheme::default(),
            checkbox: ToggleTheme::checkbox(),
            radio: ToggleTheme::radio(),
            scrollbar: ScrollbarTheme::default(),
            text_edit: TextEditTheme::default(),
            context_menu: ContextMenuTheme::default(),
            tooltip: TooltipTheme::default(),
            text: TextStyle::default(),
            window_clear: palette::TERMINAL_BG,
            panel_background: None,
            panel_clip: ClipMode::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ResponseState;
    use crate::primitives::corners::Corners;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;
    use crate::text::FontFamily;
    use crate::widgets::theme::widget_look::{AnimatedLook, WidgetLook};

    #[test]
    fn default_theme_roundtrips_through_toml() {
        let theme = Theme::default();
        let serialized = toml::to_string_pretty(&theme).expect("serialize");
        let parsed: Theme = toml::from_str(&serialized).expect("parse");
        let reserialized = toml::to_string_pretty(&parsed).expect("re-serialize");
        // Comparing serialized strings rather than `Theme == Theme`:
        // `ScrollbarTheme` deliberately doesn't derive `PartialEq`,
        // and forcing it everywhere would be theme-API churn. String
        // equality is just as strong — every field round-trips.
        assert_eq!(serialized, reserialized);
    }

    /// `WidgetLook` round-trips through TOML for both variants
    /// (background present / absent, text override / inherit).
    /// Pinned because theme files are a public surface.
    #[test]
    fn widget_look_serde_roundtrip() {
        let cases = [
            WidgetLook::default(),
            WidgetLook {
                background: Some(Background {
                    fill: Color::hex(0x336699).into(),
                    stroke: Stroke::solid(Color::hex(0xffffff), 1.5),
                    radius: Corners::all(6.0),
                    shadow: Shadow::NONE,
                }),
                text: Some(TextStyle::default().with_font_size(20.0)),
            },
        ];
        for look in cases {
            let s = toml::to_string_pretty(&look).expect("serialize");
            let back: WidgetLook = toml::from_str(&s).expect("parse");
            assert_eq!(look, back);
        }
    }

    /// `ButtonTheme::pick` precedence: disabled > pressed > hovered >
    /// normal. Table-driven sweep — every state combination resolves
    /// to the right slot, so reordering the if-cascade silently is
    /// caught.
    #[test]
    fn button_theme_pick_precedence() {
        let theme = ButtonTheme::default();
        let s = |hovered, pressed, disabled| ResponseState {
            hovered,
            pressed,
            disabled,
            ..ResponseState::default()
        };
        let cases: &[(ResponseState, &WidgetLook, &str)] = &[
            (s(false, false, false), &theme.normal, "normal"),
            (s(true, false, false), &theme.hovered, "hovered"),
            (s(true, true, false), &theme.pressed, "pressed > hovered"),
            (s(false, false, true), &theme.disabled, "disabled (idle)"),
            (s(true, true, true), &theme.disabled, "disabled wins all"),
        ];
        for (state, expected, label) in cases {
            assert!(
                std::ptr::eq(theme.pick(*state), *expected),
                "{label}: pick should return the matching slot",
            );
        }
    }

    /// `TextEditTheme::pick`: disabled > focused > normal. Reads
    /// `state.focused` from `ResponseState` (no separate parameter
    /// since `focused` is in-state now).
    #[test]
    fn text_edit_theme_pick_precedence() {
        let theme = TextEditTheme::default();
        let s = |focused, disabled| ResponseState {
            disabled,
            focused,
            ..ResponseState::default()
        };
        let cases: &[(ResponseState, &WidgetLook, &str)] = &[
            (s(false, false), &theme.normal, "normal"),
            (s(true, false), &theme.focused, "focused"),
            (s(false, true), &theme.disabled, "disabled (unfocused)"),
            (s(true, true), &theme.disabled, "disabled wins focus"),
        ];
        for (state, expected, label) in cases {
            assert!(
                std::ptr::eq(theme.pick(*state), *expected),
                "{label}: pick should return the matching slot",
            );
        }
    }

    /// Pins tooltip defaults: delay/warmup/max-width are user-facing
    /// timings, regressing them is a visible UX change.
    #[test]
    fn tooltip_theme_defaults() {
        let t = TooltipTheme::default();
        assert!((t.delay - 0.5).abs() < 1e-6);
        assert!((t.warmup - 1.0).abs() < 1e-6);
        assert!((t.max_size.w - 280.0).abs() < 1e-6);
        assert!(t.max_size.h.is_infinite());
        assert!((t.gap - 6.0).abs() < 1e-6);
    }

    /// `AnimatedLook::line_height_px` delegates to `TextStyle`'s
    /// formula (`font_size_px * line_height_mult`). Pinned because the
    /// shaper depends on it staying in sync with widget render code.
    #[test]
    fn animated_look_line_height_px_delegates_to_text_style() {
        let look = AnimatedLook {
            background: Background::default(),
            text: TextStyle {
                font_size_px: 16.0,
                color: Color::TRANSPARENT,
                line_height_mult: 1.5,
                family: FontFamily::Sans,
            },
        };
        assert!((look.line_height_px() - 24.0).abs() < 1e-6);
    }
}
