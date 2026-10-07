//! The theme bundle every widget styles from: one submodule per widget theme, over the shared [`palette`], [`text_style`] and [`widget_look`](crate::widget_core::widget_look) vocabulary.
//!
//! [`Theme`] aggregates them; a widget reads only its own slice, so a bundle grows a field without changing existing widgets.

pub(crate) mod button;
pub(crate) mod color_picker;
pub(crate) mod combo_box;
pub(crate) mod context_menu;
pub(crate) mod dock;
pub(crate) mod drag_value;
pub(crate) mod expander;
pub(crate) mod focus_ring;
pub(crate) mod modal;
pub(crate) mod palette;
pub(crate) mod progress_bar;
pub(crate) mod scrollbar;
pub(crate) mod separator;
mod serde;
pub(crate) mod slider;
pub(crate) mod spinner;
pub(crate) mod splitter;
pub(crate) mod tabs;
pub(crate) mod text_edit;
pub(crate) mod text_style;
pub(crate) mod toggle;
pub(crate) mod tooltip;

use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use crate::widgets::theme::combo_box::ComboBoxTheme;
use crate::widgets::theme::context_menu::ContextMenuTheme;
use crate::widgets::theme::dock::DockTheme;
use crate::widgets::theme::drag_value::DragValueTheme;
use crate::widgets::theme::expander::ExpanderTheme;
use crate::widgets::theme::focus_ring::FocusRingTheme;
use crate::widgets::theme::modal::ModalTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::progress_bar::ProgressBarTheme;
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use crate::widgets::theme::separator::SeparatorTheme;
use crate::widgets::theme::slider::SliderTheme;
use crate::widgets::theme::spinner::SpinnerTheme;
use crate::widgets::theme::splitter::SplitterTheme;
use crate::widgets::theme::tabs::TabsTheme;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::theme::text_style::{TextStyle, TextStyleOverrides};
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::theme::tooltip::TooltipTheme;

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
#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
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
    /// [`crate::ComboBox`] geometry; colours come from [`Self::button`] and [`Self::context_menu`].
    pub combo_box: ComboBoxTheme,
    /// Panel and backdrop of every [`crate::Modal`].
    pub modal: ModalTheme,
    /// What the colour picker and its parts wear.
    pub color_picker: ColorPickerTheme,
    /// Bubble and delay of every [`crate::Tooltip`].
    pub tooltip: TooltipTheme,
    /// Track and fill of every [`crate::ProgressBar`].
    pub progress_bar: ProgressBarTheme,
    /// Rule and margin of every [`crate::Separator`].
    pub separator: SeparatorTheme,
    /// Track, fill and knob of every [`crate::Slider`].
    pub slider: SliderTheme,
    /// Arc and speed of every [`crate::Spinner`].
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
    /// Default chrome for containers (`Panel`, `Grid`, `Popup`) without a background. `None` leaves them unpainted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_background: Option<Background>,
    /// Default clip mode for containers that set none; the rounded mask uses [`Self::panel_background`]'s `radius`.
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
            panel_background: _,
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
        tooltip.for_each_text(f);
        tabs.for_each_text(f);
        dock.for_each_text(f);
        expander.for_each_text(f);
        color_picker.for_each_text(f);
    }

    /// Assemble a full theme from a [`Palette`]. The single source of the recipes: `Theme::default()` is `from_palette(&Palette::DEFAULT)`.
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            button: ButtonTheme::from_palette(p),
            checkbox: ToggleTheme::checkbox(p),
            radio: ToggleTheme::radio(p),
            switch: ToggleTheme::switch(p),
            scrollbar: ScrollbarTheme::from_palette(p),
            text_edit: TextEditTheme::from_palette(p),
            drag_value: DragValueTheme::from_palette(p),
            context_menu: ContextMenuTheme::from_palette(p),
            combo_box: ComboBoxTheme::from_palette(p),
            modal: ModalTheme::from_palette(p),
            color_picker: ColorPickerTheme::from_palette(p),
            tooltip: TooltipTheme::from_palette(p),
            progress_bar: ProgressBarTheme::from_palette(p),
            separator: SeparatorTheme::from_palette(p),
            slider: SliderTheme::from_palette(p),
            spinner: SpinnerTheme::from_palette(p),
            splitter: SplitterTheme::from_palette(p),
            tabs: TabsTheme::from_palette(p),
            dock: DockTheme::from_palette(p),
            expander: ExpanderTheme::from_palette(p),
            focus_ring: FocusRingTheme::from_palette(p),
            text: TextStyle::default().with_color(p.text),
            window_clear: p.window_background,
            panel_background: None,
            panel_clip: ClipMode::None,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}

#[cfg(test)]
mod tests;
