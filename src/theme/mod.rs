//! The theme bundle every widget styles from: one submodule per widget theme, over the shared [`palette`], [`text_style`], [`text_style_overrides`] and [`look`] vocabulary.
//!
//! [`Theme`] aggregates them; a widget reads only its own slice, so a bundle grows a field without changing existing widgets.

pub(crate) mod button;
pub(crate) mod check_state;
pub(crate) mod close_shown;
pub(crate) mod color_picker;
pub(crate) mod color_scheme;
pub(crate) mod combo_box;
pub(crate) mod dock;
pub(crate) mod drag_value;
mod duration_codec;
pub(crate) mod expander;
pub(crate) mod focus_ring;
pub(crate) mod look;
pub(crate) mod menu;
pub(crate) mod menu_bar;
pub(crate) mod metrics;
pub(crate) mod modal;
pub(crate) mod palette;
pub(crate) mod popup;
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
use crate::primitives::math::domain;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::button::ButtonTheme;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::combo_box::ComboBoxTheme;
use crate::theme::dock::DockTheme;
use crate::theme::drag_value::DragValueTheme;
use crate::theme::expander::ExpanderTheme;
use crate::theme::focus_ring::FocusRingTheme;
use crate::theme::menu::MenuTheme;
use crate::theme::menu_bar::MenuBarTheme;
use crate::theme::metrics::Metrics;
use crate::theme::modal::ModalTheme;
use crate::theme::palette::Palette;
use crate::theme::popup::PopupTheme;
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
///
/// # Blending
///
/// [`Animatable::lerp`](crate::widget::Animatable::lerp) blends two themes as Flutter's
/// `ThemeData.lerp` does: colors, lengths, spacings, radii and font sizes blend, and a value with
/// no blend switches at `t = 0.5`. A [`Self::panel_chrome`] one end lacks blends through
/// [`Chrome::NONE`](crate::Chrome::NONE), and a text axis one end's overrides leave to
/// [`Self::text`] blends from that end's ambient value, so each step shows a blend of what the
/// ends show.
#[derive(
    Clone,
    Debug,
    PartialEq,
    ::serde::Serialize,
    ::serde::Deserialize,
    palantir_anim_derive::Animatable,
)]
#[serde(try_from = "UncheckedTheme")]
#[animate(before_lerp = Self::share_text_axes)]
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
    /// Panel and rows of every [`crate::Menu`], which every widget that drops a menu shows.
    pub menu: MenuTheme,
    /// The titles of every [`crate::MenuBar`]; its menus wear [`Self::menu`].
    pub menu_bar: MenuBarTheme,
    /// [`crate::ComboBox`] geometry; colors come from [`Self::button`] and [`Self::menu`].
    pub combo_box: ComboBoxTheme,
    /// Panel and backdrop of every [`crate::Modal`].
    pub modal: ModalTheme,
    /// Panel and padding of a [`crate::Popup`] no wrapper dresses.
    pub popup: PopupTheme,
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
    /// Default chrome for in-tree containers (`Panel`, `Grid`) without chrome of their own. `None` leaves them unpainted; a popup reads [`Self::popup`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_chrome: Option<Chrome>,
    /// Default clip mode for containers that set none; the rounded mask uses [`Self::panel_chrome`]'s `radius`.
    #[serde(skip_serializing_if = "is_clip_none")]
    #[animate(snap)]
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
const FACES_ERROR: &str = "every text slot over the theme's ambient text must make a valid face";

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
        assert!(self.faces_valid(factor), "{SCALED_TEXT_METRICS_ERROR}");
        let scale = |px: f32| px * factor;
        self.for_each_text(None, |text, _| match text {
            ThemeText::Style(style) => style.font_size = scale(style.font_size),
            ThemeText::Overrides(o) => o.font_size = o.font_size.map(scale),
        });
    }

    /// Frost every floating surface: the panels of menus and combo lists, the color picker's
    /// popup, a bare popup's panel, tooltips and the modal's panel. Each keeps its fill's color
    /// at `tint_alpha` over [`BackdropFilter::frosted`]`(sigma)`, held so the weakest ink its
    /// recipe paints reads at WCAG 4.5:1 over any backdrop: `p`'s muted ink on a menu (a
    /// shortcut), on the picker (a channel's name) and on a bare popup, which an app fills with
    /// rows like a menu's, the full ink on a tooltip and on the modal, whose content is the
    /// app's. A ceiling holds ink lighter than the tint, a floor darker. The stock themes stay opaque, as a
    /// frosted float costs a capture under each open one; an app opts in here, as WinUI's flyouts,
    /// menus and tooltips wear acrylic
    /// (<https://learn.microsoft.com/windows/apps/design/style/acrylic>) and macOS's menus and
    /// popovers vibrancy, one material for every float. Refused: a helper in each app, which a new
    /// floating slot would miss with no error.
    ///
    /// # Panics
    ///
    /// Unless `sigma` is a [length](crate::widget::domain::length), `tint_alpha` a fraction under
    /// 1, each surface's fill a solid color, each ink holds 4.5:1 on its fill made opaque, and an
    /// ink lighter than its fill is light enough for a ceiling, as
    /// [`BackdropFilter::with_max_luminance_for`] and its floor twin require.
    #[track_caller]
    pub fn frost_floats(&mut self, p: &Palette, sigma: f32, tint_alpha: f32) {
        let sigma = domain::length(sigma);
        let Self {
            menu,
            color_picker,
            tooltip,
            modal,
            popup,
            // Not floating: drawn in the tree, or a scalar.
            text: _,
            button: _,
            checkbox: _,
            radio: _,
            switch: _,
            scrollbar: _,
            text_edit: _,
            drag_value: _,
            menu_bar: _,
            combo_box: _,
            progress_bar: _,
            separator: _,
            slider: _,
            spinner: _,
            splitter: _,
            tabs: _,
            dock: _,
            expander: _,
            focus_ring: _,
            window_clear: _,
            panel_chrome: _,
            panel_clip: _,
        } = self;
        for (panel, ink) in [
            (&mut menu.panel, p.text_muted),
            (&mut color_picker.popup.panel, p.text_muted),
            (&mut popup.panel, p.text_muted),
            (&mut tooltip.panel, p.text),
            (&mut modal.panel, p.text),
        ] {
            let fill = panel
                .fill
                .as_solid()
                .expect("a floating surface to frost has a solid fill");
            let tint = fill.with_alpha(tint_alpha);
            let frosted = BackdropFilter::frosted(sigma);
            panel.backdrop_filter = if ink.luminance() > tint.luminance() {
                frosted.with_max_luminance_for(ink, tint)
            } else {
                frosted.with_min_luminance_for(ink, tint)
            };
            panel.fill = tint.into();
        }
    }

    /// Whether every text slot, its font sizes scaled by `factor`, makes a face the shaper accepts:
    /// a whole style alone, an override as the face it makes over the scaled [`Self::text`]. An
    /// override valid alone can make an invalid face, as a leading factor of 1e-6 over a 14 px
    /// ambient makes a 1.4e-5 px line. Mutable only because [`Self::for_each_text`] walks so;
    /// nothing changes.
    pub(crate) fn faces_valid(&mut self, factor: f32) -> bool {
        let scale = |px: f32| px * factor;
        let ambient = self.text.with_font_size(scale(self.text.font_size));
        let mut valid = true;
        self.for_each_text(None, |text, _| {
            valid &= match text {
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
        valid
    }

    /// Visit every text-bearing slot, each beside the same slot of `other` when one is given;
    /// [`Self::faces_valid`], [`Self::scale_text`] and [`Self::share_text_axes`] drive it.
    ///
    /// Every `for_each_text` destructures its whole struct, so a new field fails to compile until classified.
    fn for_each_text(
        &mut self,
        mut other: Option<&mut Self>,
        mut f: impl FnMut(ThemeText<'_>, Option<ThemeText<'_>>),
    ) {
        let Self {
            text,
            button,
            checkbox,
            radio,
            switch,
            text_edit,
            drag_value,
            menu,
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
            popup: _,
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
        f(
            ThemeText::Style(text),
            other.as_deref_mut().map(|o| ThemeText::Style(&mut o.text)),
        );
        button.for_each_text(other.as_deref_mut().map(|o| &mut o.button), f);
        checkbox.for_each_text(other.as_deref_mut().map(|o| &mut o.checkbox), f);
        radio.for_each_text(other.as_deref_mut().map(|o| &mut o.radio), f);
        switch.for_each_text(other.as_deref_mut().map(|o| &mut o.switch), f);
        text_edit.for_each_text(other.as_deref_mut().map(|o| &mut o.text_edit), f);
        drag_value.for_each_text(other.as_deref_mut().map(|o| &mut o.drag_value), f);
        menu.for_each_text(other.as_deref_mut().map(|o| &mut o.menu), f);
        menu_bar.for_each_text(other.as_deref_mut().map(|o| &mut o.menu_bar), f);
        tooltip.for_each_text(other.as_deref_mut().map(|o| &mut o.tooltip), f);
        tabs.for_each_text(other.as_deref_mut().map(|o| &mut o.tabs), f);
        dock.for_each_text(other.as_deref_mut().map(|o| &mut o.dock), f);
        expander.for_each_text(other.as_deref_mut().map(|o| &mut o.expander), f);
        color_picker.for_each_text(other.map(|o| &mut o.color_picker), f);
    }

    /// Give each end of a lerp the text axes only the other end's overrides name, from its own
    /// [`Self::text`]: an absent axis shows the ambient value, so the lerp blends from what each
    /// end shows, as an absent value blends through its neutral. What either end shows is
    /// unchanged. The derived `lerp` runs it.
    fn share_text_axes(a: &mut Self, b: &mut Self) {
        let (ambient_a, ambient_b) = (a.text, b.text);
        a.for_each_text(Some(b), |a, b| match (a, b) {
            (ThemeText::Overrides(a), Some(ThemeText::Overrides(b))) => {
                a.share_axes(&ambient_a, b, &ambient_b);
            }
            (ThemeText::Style(_), Some(ThemeText::Style(_))) => {}
            _ => unreachable!("two themes walk one structure"),
        });
    }

    /// Assemble a full theme from a [`Palette`]'s colors and a [`Metrics`]' steps. The single source of the recipes: [`Self::stock`] is `from_parts` over a built-in palette and `Metrics::DEFAULT`.
    ///
    /// # Panics
    ///
    /// When a text step of `m` makes a face the shaper refuses, as a `body` of 0 would.
    #[track_caller]
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let mut theme = Self {
            button: ButtonTheme::from_parts(p, m),
            checkbox: ToggleTheme::checkbox(p, m),
            radio: ToggleTheme::radio(p, m),
            switch: ToggleTheme::switch(p, m),
            scrollbar: ScrollbarTheme::from_parts(p, m),
            text_edit: TextEditTheme::from_parts(p, m),
            drag_value: DragValueTheme::from_parts(p, m),
            menu: MenuTheme::from_parts(p, m),
            menu_bar: MenuBarTheme::from_parts(p, m),
            combo_box: ComboBoxTheme::from_parts(p, m),
            modal: ModalTheme::from_parts(p, m),
            popup: PopupTheme::from_parts(p, m),
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
        };
        assert!(theme.faces_valid(1.0), "{FACES_ERROR}");
        theme
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

/// [`Theme`] as a file holds it, before the check of the whole theme: each slot checks itself
/// as it reads, and [`Theme::faces_valid`] then checks each override as the face it makes over
/// [`Theme::text`], which no slot alone can see.
#[derive(Debug, ::serde::Deserialize)]
struct UncheckedTheme {
    button: ButtonTheme,
    checkbox: ToggleTheme,
    radio: ToggleTheme,
    switch: ToggleTheme,
    scrollbar: ScrollbarTheme,
    text_edit: TextEditTheme,
    drag_value: DragValueTheme,
    menu: MenuTheme,
    menu_bar: MenuBarTheme,
    combo_box: ComboBoxTheme,
    modal: ModalTheme,
    popup: PopupTheme,
    color_picker: ColorPickerTheme,
    tooltip: TooltipTheme,
    progress_bar: ProgressBarTheme,
    separator: SeparatorTheme,
    slider: SliderTheme,
    spinner: SpinnerTheme,
    splitter: SplitterTheme,
    tabs: TabsTheme,
    dock: DockTheme,
    expander: ExpanderTheme,
    focus_ring: FocusRingTheme,
    text: TextStyle,
    window_clear: RgbaF32,
    // This and the next are left out of a file when absent, as `Theme` writes them.
    #[serde(default)]
    panel_chrome: Option<Chrome>,
    #[serde(default)]
    panel_clip: ClipMode,
}

impl TryFrom<UncheckedTheme> for Theme {
    type Error = &'static str;

    fn try_from(unchecked: UncheckedTheme) -> Result<Self, Self::Error> {
        let UncheckedTheme {
            button,
            checkbox,
            radio,
            switch,
            scrollbar,
            text_edit,
            drag_value,
            menu,
            menu_bar,
            combo_box,
            modal,
            popup,
            color_picker,
            tooltip,
            progress_bar,
            separator,
            slider,
            spinner,
            splitter,
            tabs,
            dock,
            expander,
            focus_ring,
            text,
            window_clear,
            panel_chrome,
            panel_clip,
        } = unchecked;
        let mut theme = Self {
            button,
            checkbox,
            radio,
            switch,
            scrollbar,
            text_edit,
            drag_value,
            menu,
            menu_bar,
            combo_box,
            modal,
            popup,
            color_picker,
            tooltip,
            progress_bar,
            separator,
            slider,
            spinner,
            splitter,
            tabs,
            dock,
            expander,
            focus_ring,
            text,
            window_clear,
            panel_chrome,
            panel_clip,
        };
        if !theme.faces_valid(1.0) {
            return Err(FACES_ERROR);
        }
        Ok(theme)
    }
}

#[cfg(test)]
mod tests;
