//! The theme bundle every widget styles from: one submodule per widget theme, over the shared [`palette`], [`text_style`], [`text_style_overrides`] and [`look`] vocabulary.
//!
//! [`Theme`] aggregates them; a widget reads only its own slice, so a bundle grows a field without changing existing widgets.

pub(crate) mod button;
pub(crate) mod check_state;
pub(crate) mod close_shown;
pub(crate) mod color_picker;
pub(crate) mod color_scheme;
pub(crate) mod combo_box;
pub(crate) mod context_menu;
pub(crate) mod dock;
pub(crate) mod drag_value;
mod duration_codec;
pub(crate) mod expander;
pub(crate) mod focus_ring;
pub(crate) mod look;
pub(crate) mod menu_bar;
pub(crate) mod metrics;
pub(crate) mod modal;
pub(crate) mod palette;
pub(crate) mod progress_bar;
pub(crate) mod scheme_preference;
pub(crate) mod scrollbar;
pub(crate) mod selection;
pub(crate) mod separator;
pub(crate) mod slider;
pub(crate) mod spinner;
pub(crate) mod splitter;
pub(crate) mod tab_strip_focus;
pub(crate) mod tabs;
pub(crate) mod text_edit;
pub(crate) mod text_style;
pub(crate) mod text_style_overrides;
pub(crate) mod toggle;
pub(crate) mod tooltip;

use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::button::ButtonTheme;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::combo_box::ComboBoxTheme;
use crate::theme::context_menu::ContextMenuTheme;
use crate::theme::dock::DockTheme;
use crate::theme::drag_value::DragValueTheme;
use crate::theme::expander::ExpanderTheme;
use crate::theme::focus_ring::FocusRingTheme;
use crate::theme::menu_bar::MenuBarTheme;
use crate::theme::metrics::Metrics;
use crate::theme::modal::ModalTheme;
use crate::theme::palette::Palette;
use crate::theme::progress_bar::ProgressBarTheme;
use crate::theme::scrollbar::ScrollbarTheme;
use crate::theme::separator::SeparatorTheme;
use crate::theme::slider::SliderTheme;
use crate::theme::spinner::SpinnerTheme;
use crate::theme::splitter::SplitterTheme;
use crate::theme::tabs::TabsTheme;
use crate::theme::text_edit::TextEditTheme;
use crate::theme::text_style::TextStyle;
use crate::theme::text_style_overrides::TextStyleOverrides;
use crate::theme::toggle::ToggleTheme;
use crate::theme::tooltip::TooltipTheme;

/// Global theme, aggregating per-widget themes. Widgets read it from `Ui::theme`.
///
/// # Overriding a widget's look
///
/// Every themed widget takes `.style(&XTheme)`, which replaces its whole bundle for that call. To move one axis, build from the theme: `SpinnerTheme { color: red, ..ui.theme().spinner.clone() }`.
///
/// Some widgets expose one-axis hatches (e.g. [`Separator::color`](crate::Separator::color), [`Text::bold`](crate::Text::bold)): an `Option<T>` merged over the resolved bundle at `show()`, for per-call properties of one occurrence. An axis set the same way everywhere belongs in the bundle.
///
/// # Disabled state
///
/// The framework does not auto-dim disabled subtrees; widgets read the disabled flag themselves.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct Theme {
    /// What every [`crate::Button`] wears; [`crate::ComboBox`] and [`crate::DragValue`] derive from it.
    pub button: ButtonTheme,
    /// Toggle widgets share a theme type, not a slot.
    pub checkbox: ToggleTheme,
    /// See [`Self::checkbox`].
    pub radio: ToggleTheme,
    /// See [`Self::checkbox`].
    pub switch: ToggleTheme,
    /// Track, thumb and gutter of every [`crate::Scroll`].
    pub scrollbar: ScrollbarTheme,
    /// What every [`crate::TextEdit`] wears; [`Self::drag_value`]'s editor derives from it.
    pub text_edit: TextEditTheme,
    /// [`crate::DragValue`]'s chip and editor, derived from `button` + `text_edit` via [`DragValueTheme::from_chip`].
    pub drag_value: DragValueTheme,
    /// Panel and rows of every [`crate::ContextMenu`] and [`crate::ComboBox`] dropdown.
    pub context_menu: ContextMenuTheme,
    /// The titles of every [`crate::MenuBar`]; its menus wear [`Self::context_menu`].
    pub menu_bar: MenuBarTheme,
    /// [`crate::ComboBox`] geometry; colors come from [`Self::button`] and [`Self::context_menu`].
    pub combo_box: ComboBoxTheme,
    /// Panel and backdrop of every [`crate::Modal`].
    pub modal: ModalTheme,
    /// What the color picker and its parts wear.
    pub color_picker: ColorPickerTheme,
    /// Bubble and delay of every [`crate::Tooltip`].
    pub tooltip: TooltipTheme,
    /// Track and fill of every [`crate::ProgressBar`].
    pub progress_bar: ProgressBarTheme,
    /// Rule and margin of every [`crate::Separator`].
    pub separator: SeparatorTheme,
    /// Track, fill and knob of every [`crate::Slider`].
    pub slider: SliderTheme,
    /// Arc and period of every [`crate::Spinner`].
    pub spinner: SpinnerTheme,
    /// Grab band and rule of every [`crate::Splitter`].
    pub splitter: SplitterTheme,
    /// What every [`crate::TabStrip`], [`crate::TabbedView`] and [`crate::DockView`] pane wears.
    pub tabs: TabsTheme,
    /// [`crate::DockView`]'s drop preview and trailing chip; dividers read [`Self::splitter`], panes [`Self::tabs`].
    pub dock: DockTheme,
    /// [`crate::Expander`] header and body inset.
    pub expander: ExpanderTheme,
    /// Ring around the keyboard-focused widget.
    pub focus_ring: FocusRingTheme,
    /// Ambient text style every [`Text`](crate::Text) falls back to; other text slots are [`TextStyleOverrides`] over this.
    pub text: TextStyle,
    /// Window/swapchain clear color, passed to `WgpuBackend::submit`.
    pub window_clear: RgbaF32,
    /// Default chrome for containers (`Panel`, `Grid`, `Popup`) without chrome of their own. `None` leaves them unpainted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_chrome: Option<Chrome>,
    /// Default clip mode for containers that set none; the rounded mask uses [`Self::panel_chrome`]'s `radius`.
    #[serde(default, skip_serializing_if = "is_clip_none")]
    pub panel_clip: ClipMode,
}

/// One text-bearing slot of a theme, as [`Theme::for_each_text`] hands it over: a whole style, or a look's overrides folded onto [`Theme::text`].
#[derive(Debug)]
pub(crate) enum ThemeText<'a> {
    Style(&'a mut TextStyle),
    Overrides(&'a mut TextStyleOverrides),
}

const TEXT_SCALE_ERROR: &str = "text scale factor must be finite and positive";
const SCALED_TEXT_METRICS_ERROR: &str = "text scale would make font size or line height invalid";

#[inline]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` passes the field by reference"
)]
const fn is_clip_none(c: &ClipMode) -> bool {
    matches!(c, ClipMode::None)
}

#[inline]
fn text_scale_is_valid(scale: f32) -> bool {
    scale.is_finite() && scale > 0.0
}

impl Theme {
    /// Multiply every font size in the theme by `factor`. Relative and composing (`1.25` then `1.6` lands at 2.0×); colors, spacing and chrome are untouched.
    ///
    /// # Panics
    ///
    /// Panics if `factor` is not finite and positive, or would push a font size or line height outside what the shaper accepts. Checked before the first write, so a rejection leaves the theme untouched.
    pub fn scale_text(&mut self, factor: f32) {
        assert!(text_scale_is_valid(factor), "{TEXT_SCALE_ERROR}");
        let scale = |px: f32| px * factor;
        let ambient = self.text.with_font_size(scale(self.text.font_size));
        let mut metrics_valid = true;
        self.for_each_text(|text| {
            metrics_valid &= match text {
                ThemeText::Style(style) => {
                    style.with_font_size(scale(style.font_size)).metrics_valid()
                }
                ThemeText::Overrides(o) => TextStyleOverrides {
                    font_size: o.font_size.map(scale),
                    ..*o
                }
                .apply(&ambient)
                .metrics_valid(),
            };
        });
        assert!(metrics_valid, "{SCALED_TEXT_METRICS_ERROR}");
        self.for_each_text(|text| match text {
            ThemeText::Style(style) => style.font_size = scale(style.font_size),
            ThemeText::Overrides(o) => o.font_size = o.font_size.map(scale),
        });
    }

    /// Visit every text-bearing slot; [`Self::scale_text`] drives it.
    ///
    /// Every `for_each_text` destructures its whole struct, so a new field fails to compile until classified.
    fn for_each_text(&mut self, mut f: impl FnMut(ThemeText<'_>)) {
        let Self {
            text,
            button,
            checkbox,
            radio,
            switch,
            text_edit,
            drag_value,
            context_menu,
            menu_bar,
            tooltip,
            tabs,
            dock,
            expander,
            color_picker,
            // Chrome, geometry and scalars: no text slot.
            scrollbar: _,
            combo_box: _,
            modal: _,
            progress_bar: _,
            separator: _,
            slider: _,
            spinner: _,
            splitter: _,
            focus_ring: _,
            window_clear: _,
            panel_chrome: _,
            panel_clip: _,
        } = self;
        let f = &mut f;
        f(ThemeText::Style(text));
        button.for_each_text(f);
        checkbox.for_each_text(f);
        radio.for_each_text(f);
        switch.for_each_text(f);
        text_edit.for_each_text(f);
        drag_value.for_each_text(f);
        context_menu.for_each_text(f);
        menu_bar.for_each_text(f);
        tooltip.for_each_text(f);
        tabs.for_each_text(f);
        dock.for_each_text(f);
        expander.for_each_text(f);
        color_picker.for_each_text(f);
    }

    /// Assemble a full theme from a [`Palette`]'s colors and a [`Metrics`]' steps. The single source of the recipes: [`Self::stock`] is `from_parts` over a built-in palette and `Metrics::DEFAULT`.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            button: ButtonTheme::from_parts(p, m),
            checkbox: ToggleTheme::checkbox(p, m),
            radio: ToggleTheme::radio(p, m),
            switch: ToggleTheme::switch(p, m),
            scrollbar: ScrollbarTheme::from_parts(p, m),
            text_edit: TextEditTheme::from_parts(p, m),
            drag_value: DragValueTheme::from_parts(p, m),
            context_menu: ContextMenuTheme::from_parts(p, m),
            menu_bar: MenuBarTheme::from_parts(p, m),
            combo_box: ComboBoxTheme::from_parts(p, m),
            modal: ModalTheme::from_parts(p, m),
            color_picker: ColorPickerTheme::from_parts(p, m),
            tooltip: TooltipTheme::from_parts(p, m),
            progress_bar: ProgressBarTheme::from_parts(p, m),
            separator: SeparatorTheme::from_parts(p, m),
            slider: SliderTheme::from_parts(p, m),
            spinner: SpinnerTheme::from_parts(p, m),
            splitter: SplitterTheme::from_parts(p, m),
            tabs: TabsTheme::from_parts(p, m),
            dock: DockTheme::from_parts(p, m),
            expander: ExpanderTheme::from_parts(p, m),
            focus_ring: FocusRingTheme::from_parts(p, m),
            text: TextStyle::default()
                .with_font_size(m.text.body)
                .with_color(p.text),
            window_clear: p.window_bg,
            panel_chrome: None,
            panel_clip: ClipMode::None,
        }
    }

    /// The built-in look of `scheme`: its built-in palette over [`Metrics::DEFAULT`].
    pub fn stock(scheme: ColorScheme) -> Self {
        Self::from_parts(Palette::for_scheme(scheme), &Metrics::DEFAULT)
    }
}

impl Default for Theme {
    /// The dark stock look, [`Self::stock`] of [`ColorScheme::Dark`].
    fn default() -> Self {
        Self::stock(ColorScheme::Dark)
    }
}

#[cfg(test)]
mod tests;
