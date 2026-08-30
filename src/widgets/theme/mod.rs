//! The theme bundle every widget styles from: one submodule per widget's
//! own theme, over the shared [`palette`], [`text_style`] and
//! [`widget_look`] vocabulary they are all built out of.
//!
//! [`Theme`] aggregates them. A widget opts in by reading its own slice,
//! so a bundle grows a field without any existing widget changing.

/// `Default` for a theme bundle whose default *is* the default palette.
///
///
/// Every bundle in here builds from a [`palette::Palette`], and the
/// stock look is that recipe over [`palette::Palette::DEFAULT`] — so the
/// impl is the same line each time and only the type varies. Invoke it
/// **in the bundle's own file**, next to `from_palette`.
macro_rules! palette_default {
    ($ty:ty) => {
        impl Default for $ty {
            fn default() -> Self {
                Self::from_palette(&$crate::widgets::theme::palette::Palette::DEFAULT)
            }
        }
    };
}

pub(crate) mod button;
pub(crate) mod combo_box;
pub(crate) mod context_menu;
pub(crate) mod drag_value;
pub(crate) mod modal;
pub(crate) mod palette;
pub(crate) mod progress_bar;
pub(crate) mod scrollbar;
pub(crate) mod separator;
mod serde;
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
use crate::scene::node::container_chrome::ContainerChrome;
use crate::text::glyph_font::GlyphFont;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::combo_box::ComboBoxTheme;
use crate::widgets::theme::context_menu::ContextMenuTheme;
use crate::widgets::theme::drag_value::DragValueTheme;
use crate::widgets::theme::modal::ModalTheme;
use crate::widgets::theme::palette::Palette;
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
/// # Overriding a widget's look
///
/// Every themed widget takes `.style(&XTheme)`, which replaces its whole
/// bundle for that call. It is all-or-nothing by design — to move one
/// axis, build the bundle from the theme:
/// `SpinnerTheme { color: red, ..ui.theme().spinner.clone() }`.
///
/// Some widgets additionally expose **one-axis hatches** —
/// [`Separator::color`](crate::Separator::color) /
/// [`thickness`](crate::Separator::thickness),
/// [`Spinner::color`](crate::Spinner::color) /
/// [`diameter`](crate::Spinner::diameter) /
/// [`thickness`](crate::Spinner::thickness),
/// [`Modal::backdrop`](crate::Modal::backdrop), and
/// [`Text::bold`](crate::Text::bold). They are not a second styling
/// system: each is an `Option<T>` merged over the *resolved* bundle at
/// `show()`, so it composes with `.style(...)` rather than competing
/// with it, and leaving it unset changes nothing.
///
/// The rule for adding one: the axis is a per-call property of *this*
/// occurrence rather than of the app's look — one rule in a stack drawn
/// heavier, one word in a paragraph bolded, one modal's scrim darkened.
/// An axis a caller would set the same way everywhere belongs in the
/// bundle, where it can be set once. A widget with no hatches simply has
/// no axis that passes the test.
///
/// # Disabled state
///
/// The framework does not auto-dim disabled subtrees — that's an
/// app/theme concern. Widgets that want disabled-state visuals read the
/// disabled flag themselves and pick their own colors at recording
/// time.
#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct Theme {
    pub button: ButtonTheme,
    /// The three toggle widgets share a theme *type* but not a *slot* —
    /// restyling one leaves the other two alone.
    pub checkbox: ToggleTheme,
    /// See [`Self::checkbox`].
    pub radio: ToggleTheme,
    /// See [`Self::checkbox`].
    pub switch: ToggleTheme,
    pub scrollbar: ScrollbarTheme,
    pub text_edit: TextEditTheme,
    /// Theme for [`crate::DragValue`] — the scrub chip plus its inline
    /// editor. Both modes resolve from this bundle (`chip` at rest,
    /// `editor` while editing), so restyling it moves them together.
    /// The default derives both from `button` + `text_edit` via
    /// [`DragValueTheme::from_chip`]; apps that restyle `button` and
    /// want DragValue to match should rebuild this bundle the same way.
    pub drag_value: DragValueTheme,
    pub context_menu: ContextMenuTheme,
    /// Geometry for [`crate::ComboBox`]; its colours come from
    /// [`Self::button`] and [`Self::context_menu`].
    pub combo_box: ComboBoxTheme,
    pub modal: ModalTheme,
    pub tooltip: TooltipTheme,
    pub progress_bar: ProgressBarTheme,
    pub separator: SeparatorTheme,
    pub slider: SliderTheme,
    pub spinner: SpinnerTheme,
    pub splitter: SplitterTheme,
    /// Ambient text style — size, colour, family, leading — that every
    /// [`Text`](crate::Text) falls back to when its builder didn't
    /// override the axis, and that a widget look inherits whole wherever
    /// its `text` slot is `None`. A state-styled widget overrides it by
    /// filling that slot, which is all or nothing.
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
}

const TEXT_SCALE_ERROR: &str = "text scale factor must be finite and positive";
const SCALED_TEXT_METRICS_ERROR: &str = "text scale would make font size or line height invalid";

#[inline]
fn is_clip_none(c: &ClipMode) -> bool {
    matches!(c, ClipMode::None)
}

#[inline]
fn text_scale_is_valid(scale: f32) -> bool {
    scale.is_finite() && scale > 0.0
}

impl Theme {
    /// The chrome a container falls back to when the caller named
    /// none — [`Self::panel_background`] and [`Self::panel_clip`] as
    /// one value, so no container writes down which two fields the
    /// fallback is.
    pub(crate) fn container_chrome(&self) -> ContainerChrome<'_> {
        ContainerChrome {
            background: self.panel_background.as_ref(),
            clip: self.panel_clip,
        }
    }

    /// Multiply every `TextStyle` in the theme by `factor`.
    ///
    /// **Relative, and it composes**: `scale_text(1.25)` then
    /// `scale_text(1.6)` lands at 2.0×. The theme stores font sizes and
    /// nothing else — there is no scale factor beside them to fall out of
    /// step with, which is why this is a multiply rather than an absolute
    /// target. An app offering the user a "125% text" setting keeps that
    /// number itself and applies it to a freshly built theme, the same way
    /// it keeps which palette it built from.
    ///
    /// Affects only font sizes; colors / spacing / chrome are untouched.
    ///
    /// # Panics
    ///
    /// Panics if `factor` is not finite and positive, or if it would drive
    /// any font size or line height outside the range the shaper accepts.
    /// Both checks run before the first write, so a rejected factor leaves
    /// the theme untouched.
    pub fn scale_text(&mut self, factor: f32) {
        assert!(text_scale_is_valid(factor), "{TEXT_SCALE_ERROR}");
        let mut metrics_valid = true;
        self.for_each_text(|style| {
            let font_size_px = style.font_size_px * factor;
            metrics_valid &=
                GlyphFont::metrics_are_valid(font_size_px, style.line_height_for(font_size_px));
        });
        assert!(metrics_valid, "{SCALED_TEXT_METRICS_ERROR}");
        self.for_each_text(|t| t.font_size_px *= factor);
    }

    /// Visit every `TextStyle` in the theme. [`Self::scale_text`] drives
    /// the walk; each sub-theme owns its own visit (see each
    /// `for_each_text`).
    ///
    /// **Every `for_each_text` in this module destructures its whole
    /// struct**, binding the text-free fields to `_`, so a new field
    /// anywhere in the theme tree fails to compile here until someone
    /// classifies it as text-bearing or not. That is the guarantee; the
    /// runtime backstop is
    /// `tests::text_scale::scale_text_reaches_every_font_size`,
    /// which scales a default theme and asserts over its serialized
    /// form that every `font_size_px` moved. The test can only see
    /// styles the default theme materializes — an `Option<TextStyle>`
    /// left `None` by default is invisible to it — which is exactly the
    /// gap the destructuring closes.
    fn for_each_text(&mut self, mut f: impl FnMut(&mut TextStyle)) {
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
            // Chrome, geometry, and scalars — no `TextStyle` reachable.
            scrollbar: _,
            combo_box: _,
            modal: _,
            progress_bar: _,
            separator: _,
            slider: _,
            spinner: _,
            splitter: _,
            window_clear: _,
            panel_background: _,
            panel_clip: _,
        } = self;
        let f = &mut f;
        f(text);
        button.for_each_text(f);
        checkbox.for_each_text(f);
        radio.for_each_text(f);
        switch.for_each_text(f);
        text_edit.for_each_text(f);
        drag_value.for_each_text(f);
        context_menu.for_each_text(f);
        tooltip.for_each_text(f);
    }

    /// Assemble a full theme from a [`Palette`] — every widget recipe
    /// recolored from one roster. This is the single source of the
    /// recipes: `Theme::default()` is `from_palette(&Palette::DEFAULT)`,
    /// and apps with their own palettes (light themes, brand colors)
    /// build here instead of hand-recoloring each sub-theme.
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
            tooltip: TooltipTheme::from_palette(p),
            progress_bar: ProgressBarTheme::from_palette(p),
            separator: SeparatorTheme::from_palette(p),
            slider: SliderTheme::from_palette(p),
            spinner: SpinnerTheme::from_palette(p),
            splitter: SplitterTheme::from_palette(p),
            text: TextStyle::default().with_color(p.text),
            window_clear: p.terminal_bg,
            panel_background: None,
            panel_clip: ClipMode::None,
        }
    }
}

palette_default!(Theme);

#[cfg(test)]
mod tests;
