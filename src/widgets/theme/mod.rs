pub(crate) mod button;
pub(crate) mod context_menu;
pub(crate) mod drag_value;
pub(crate) mod modal;
pub(crate) mod palette;
pub(crate) mod progress_bar;
pub(crate) mod scrollbar;
pub(crate) mod separator;
pub(crate) mod slider;
pub(crate) mod spinner;
pub(crate) mod splitter;
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
use crate::widgets::theme::drag_value::DragValueTheme;
use crate::widgets::theme::modal::ModalTheme;
use crate::widgets::theme::progress_bar::ProgressBarTheme;
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use crate::widgets::theme::separator::SeparatorTheme;
use crate::widgets::theme::slider::SliderTheme;
use crate::widgets::theme::spinner::SpinnerTheme;
use crate::widgets::theme::splitter::SplitterTheme;
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
    /// Theme slot for `Button`s used as menu-bar triggers — flat,
    /// hover-on-only, opens a popup on click. Distinct from `button`
    /// so apps can restyle one without affecting in-flow buttons,
    /// and from `context_menu.item` which is for *rows inside* the
    /// popup. Default built by [`ButtonTheme::menu_button`].
    pub menu_button: ButtonTheme,
    pub checkbox: ToggleTheme,
    pub radio: ToggleTheme,
    pub switch: ToggleTheme,
    pub scrollbar: ScrollbarTheme,
    pub text_edit: TextEditTheme,
    /// Theme for [`crate::DragValue`] — the scrub chip plus its inline editor.
    /// Default derives the editor from `button` + `text_edit` so the two modes
    /// match; apps that restyle either should rebuild this to stay consistent.
    pub drag_value: DragValueTheme,
    pub context_menu: ContextMenuTheme,
    pub modal: ModalTheme,
    pub tooltip: TooltipTheme,
    pub progress_bar: ProgressBarTheme,
    pub separator: SeparatorTheme,
    pub slider: SliderTheme,
    pub spinner: SpinnerTheme,
    pub splitter: SplitterTheme,
    pub text: TextStyle,
    /// Window/swapchain clear color. Hosts pass to `WgpuBackend::submit`.
    pub window_clear: Color,
    /// Default chrome paint for container widgets (`Panel`, `Grid`,
    /// `Popup`) that didn't set their own background.
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
    /// Global text-size multiplier (1.0 = unscaled). Read-only — it's
    /// kept in sync with the stored font sizes, which are *already
    /// scaled*. Change it through [`Theme::set_text_scale`]; a direct
    /// write would desync the recorded sizes from the factor.
    #[serde(default = "default_text_scale")]
    text_scale: f32,
}

#[inline]
fn is_clip_none(c: &ClipMode) -> bool {
    matches!(c, ClipMode::None)
}

#[inline]
fn default_text_scale() -> f32 {
    1.0
}

impl Theme {
    /// Current global text scale (1.0 = unscaled).
    #[inline]
    pub fn text_scale(&self) -> f32 {
        self.text_scale
    }

    /// Set the global text scale, rescaling every `TextStyle` in the
    /// theme by the delta from the current scale (`new / old`). So
    /// `set_text_scale(1.25)` then `set_text_scale(2.0)` ends at a 2.0×
    /// size (not 2.5×) — it's an absolute target, not cumulative.
    /// Affects only font sizes; colors / spacing / chrome are
    /// untouched. The theme is the single owner of this; widgets read
    /// the already-scaled sizes and know nothing about the factor.
    pub fn set_text_scale(&mut self, scale: f32) {
        assert!(
            scale.is_finite() && scale > 0.0,
            "text scale must be finite and positive"
        );
        let ratio = scale / self.text_scale;
        self.text_scale = scale;
        self.for_each_text(|t| t.font_size_px *= ratio);
    }

    /// Visit every `TextStyle` in the theme. `set_text_scale` drives the
    /// walk; each sub-theme owns its own visit (see each `for_each_text`),
    /// so adding a font-bearing field updates the walk in that field's own
    /// file rather than silently escaping this one.
    fn for_each_text(&mut self, mut f: impl FnMut(&mut TextStyle)) {
        let f = &mut f;
        f(&mut self.text);
        self.button.for_each_text(f);
        self.menu_button.for_each_text(f);
        self.checkbox.for_each_text(f);
        self.radio.for_each_text(f);
        self.switch.for_each_text(f);
        self.text_edit.for_each_text(f);
        self.drag_value.for_each_text(f);
        self.context_menu.for_each_text(f);
        f(&mut self.tooltip.text);
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            button: ButtonTheme::default(),
            menu_button: ButtonTheme::menu_button(),
            checkbox: ToggleTheme::checkbox(),
            radio: ToggleTheme::radio(),
            switch: ToggleTheme::switch(),
            scrollbar: ScrollbarTheme::default(),
            text_edit: TextEditTheme::default(),
            drag_value: DragValueTheme::default(),
            context_menu: ContextMenuTheme::default(),
            modal: ModalTheme::default(),
            tooltip: TooltipTheme::default(),
            progress_bar: ProgressBarTheme::default(),
            separator: SeparatorTheme::default(),
            slider: SliderTheme::default(),
            spinner: SpinnerTheme::default(),
            splitter: SplitterTheme::default(),
            text: TextStyle::default(),
            window_clear: palette::TERMINAL_BG,
            panel_background: None,
            panel_clip: ClipMode::None,
            text_scale: default_text_scale(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::response::ResponseState;
    use crate::primitives::corners::Corners;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;
    use crate::text::{FontFamily, FontWeight};
    use crate::widgets::theme::widget_look::{AnimatedLook, WidgetLook};
    use crate::widgets::theme::*;

    /// `set_text_scale` multiplies every font size by `new/old` (so
    /// it's an absolute target, not cumulative), touches both the
    /// inherited `Theme::text` and explicit overrides (tooltip /
    /// disabled looks), and round-trips back to the originals at 1.0.
    #[test]
    fn set_text_scale_is_absolute_and_total() {
        let mut t = Theme::default();
        let body = t.text.font_size_px; // 16
        let tip = t.tooltip.text.font_size_px; // 13
        // Button disabled is an explicit `Some(TextStyle)` override.
        let disabled = t
            .button
            .disabled
            .text
            .expect("button disabled has a text override")
            .font_size_px;

        t.set_text_scale(2.0);
        assert_eq!(t.text_scale(), 2.0);
        assert!((t.text.font_size_px - body * 2.0).abs() < 1e-3);
        assert!((t.tooltip.text.font_size_px - tip * 2.0).abs() < 1e-3);
        assert!((t.button.disabled.text.unwrap().font_size_px - disabled * 2.0).abs() < 1e-3);

        // Absolute: 2.0 → 1.5 lands at 1.5×, not 3.0×.
        t.set_text_scale(1.5);
        assert_eq!(t.text_scale(), 1.5);
        assert!((t.text.font_size_px - body * 1.5).abs() < 1e-3);

        // Back to 1.0 restores the originals.
        t.set_text_scale(1.0);
        assert!((t.text.font_size_px - body).abs() < 1e-3);
        assert!((t.tooltip.text.font_size_px - tip).abs() < 1e-3);
        assert!((t.button.disabled.text.unwrap().font_size_px - disabled).abs() < 1e-3);
    }

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
                    corners: Corners::all(6.0),
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
                weight: FontWeight::Regular,
            },
        };
        assert!((look.line_height_px() - 24.0).abs() < 1e-6);
    }
}
