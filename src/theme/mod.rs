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
pub(crate) mod segmented;
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
pub(crate) mod toggle_kind;
pub(crate) mod tooltip;

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::button::ButtonTheme;
use crate::theme::close_shown::CloseShown;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::combo_box::ComboBoxTheme;
use crate::theme::dock::DockTheme;
use crate::theme::drag_value::DragValueTheme;
use crate::theme::expander::ExpanderTheme;
use crate::theme::focus_ring::FocusRingTheme;
use crate::theme::menu::MenuTheme;
use crate::theme::menu_bar::MenuBarTheme;
use crate::theme::metrics::{FloatMaterial, Metrics};
use crate::theme::modal::ModalTheme;
use crate::theme::palette::Palette;
use crate::theme::popup::PopupTheme;
use crate::theme::progress_bar::ProgressBarTheme;
use crate::theme::scrollbar::ScrollbarTheme;
use crate::theme::segmented::SegmentedTheme;
use crate::theme::separator::SeparatorTheme;
use crate::theme::slider::SliderTheme;
use crate::theme::spinner::SpinnerTheme;
use crate::theme::splitter::SplitterTheme;
use crate::theme::tabs::TabsTheme;
use crate::theme::text_edit::TextEditTheme;
use crate::theme::text_style::TextStyle;
use crate::theme::text_style_overrides::TextStyleOverrides;
use crate::theme::toggle::ToggleTheme;
use crate::theme::toggle_kind::ToggleKind;
use crate::theme::tooltip::TooltipTheme;
use glam::Vec2;
use std::time::Duration;

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
/// [`Lerp::lerp`](crate::widget::Lerp::lerp) blends two themes as Flutter's
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
    palantir_anim_derive::Lerp,
    palantir_anim_derive::TextSlots,
)]
#[serde(try_from = "UncheckedTheme")]
#[animate(before_lerp = Self::share_text_axes)]
pub struct Theme {
    /// What every [`crate::Button`] wears; [`crate::ComboBox`] and [`crate::DragValue`] derive from it.
    pub button: ButtonTheme,
    /// Toggle widgets share a theme type, not a slot.
    pub checkbox: ToggleTheme,
    /// The pips of a [`crate::RadioGroup`]; see [`Self::checkbox`].
    pub radio: ToggleTheme,
    /// The track and segments of a [`crate::RadioGroup`] in
    /// [`RadioLook::Segment`](crate::RadioLook::Segment).
    pub segmented: SegmentedTheme,
    /// See [`Self::checkbox`].
    pub switch: ToggleTheme,
    /// Track, thumb and gutter of every [`crate::Scroll`].
    pub scrollbar: ScrollbarTheme,
    /// What every [`crate::TextEdit`] wears; a stock [`Self::drag_value`] takes its caret,
    /// selection and placeholder.
    pub text_edit: TextEditTheme,
    /// [`crate::DragValue`]'s chip, and the editor it becomes ([`DragValueTheme::editor`]).
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
    /// The window's background, which each frame clears to; [`Palette::window_bg`] in a stock
    /// theme.
    pub window_bg: RgbaF32,
    /// Default chrome for in-tree containers (`Panel`, `Grid`) without chrome of their own. `None` leaves them unpainted; a popup reads [`Self::popup`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_chrome: Option<Chrome>,
    /// Default clip mode for containers that set none; the rounded mask uses [`Self::panel_chrome`]'s `radius`.
    #[serde(skip_serializing_if = "is_clip_none")]
    #[animate(snap)]
    pub panel_clip: ClipMode,
}

/// One text-bearing slot of a theme, as [`TextSlots::for_each_text`] hands it over: a whole style, or a look's overrides folded onto [`Theme::text`].
#[derive(Debug)]
pub(crate) enum ThemeText<'a> {
    Style(&'a mut TextStyle),
    Overrides(&'a mut TextStyleOverrides),
}

/// [`ThemeText`] to read, as [`TextSlots::each_text`] hands it over.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ThemeTextRef<'a> {
    Style(&'a TextStyle),
    Overrides(&'a TextStyleOverrides),
}

/// A theme part's text slots: [`Self::for_each_text`] visits each, beside the same slot of
/// `other` when one is given, in field order. [`Theme::faces_valid`], [`Theme::scale_text`] and
/// [`Theme::share_text_axes`] drive it. A part derives it and walks every field, so each field's
/// type implements it, a value that holds no text through `no_text_slots!`, and a field of a new
/// type fails to compile until its type says whether it holds text.
pub(crate) trait TextSlots {
    /// Each text slot, in field order, to read.
    fn each_text<F>(&self, f: &mut F)
    where
        F: FnMut(ThemeTextRef<'_>);

    fn for_each_text<F>(&mut self, other: Option<&mut Self>, f: &mut F)
    where
        F: FnMut(ThemeText<'_>, Option<ThemeText<'_>>);
}

/// A walk that visits nothing, for each value type that holds no text slot.
macro_rules! no_text_slots {
    ($($ty:ty),* $(,)?) => {$(
        impl TextSlots for $ty {
            #[inline]
            fn each_text<F>(&self, _: &mut F)
            where
                F: FnMut(ThemeTextRef<'_>),
            {
            }

            #[inline]
            fn for_each_text<F>(&mut self, _: Option<&mut Self>, _: &mut F)
            where
                F: FnMut(ThemeText<'_>, Option<ThemeText<'_>>),
            {
            }
        }
    )*};
}

no_text_slots!(
    f32,
    Vec2,
    [Vec2; 3],
    Size,
    Spacing,
    Duration,
    RgbaF32,
    Chrome,
    Option<Chrome>,
    AnimationSpec,
    ClipMode,
    CloseShown,
);

/// A whole style.
impl TextSlots for TextStyle {
    fn each_text<F>(&self, f: &mut F)
    where
        F: FnMut(ThemeTextRef<'_>),
    {
        f(ThemeTextRef::Style(self));
    }

    fn for_each_text<F>(&mut self, other: Option<&mut Self>, f: &mut F)
    where
        F: FnMut(ThemeText<'_>, Option<ThemeText<'_>>),
    {
        f(ThemeText::Style(self), other.map(ThemeText::Style));
    }
}

/// A look's overrides, folded onto [`Theme::text`].
impl TextSlots for TextStyleOverrides {
    fn each_text<F>(&self, f: &mut F)
    where
        F: FnMut(ThemeTextRef<'_>),
    {
        f(ThemeTextRef::Overrides(self));
    }

    fn for_each_text<F>(&mut self, other: Option<&mut Self>, f: &mut F)
    where
        F: FnMut(ThemeText<'_>, Option<ThemeText<'_>>),
    {
        f(ThemeText::Overrides(self), other.map(ThemeText::Overrides));
    }
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
        self.for_each_text(None, &mut |text, _| match text {
            ThemeText::Style(style) => style.font_size = scale(style.font_size),
            ThemeText::Overrides(o) => o.font_size = o.font_size.map(scale),
        });
    }

    /// Dress every floating surface in `material`: the panels of menus and combo lists, the color
    /// picker's popup, a bare popup's panel, tooltips and the modal's panel. Each keeps its
    /// fill's color at the material's tint over its filter, blurred, frosted or glass, held so the
    /// weakest ink its recipe paints reads at WCAG 4.5:1 over any backdrop, read from this theme:
    /// a menu row's shortcut ink on a menu and on a bare popup, which an app fills with rows like
    /// a menu's, the picker's channel-name ink, the tooltip's text, and the ambient text on the
    /// modal, whose content is the app's.
    /// The stock themes dress them already in [`Metrics::float_material`]; an app dresses them in
    /// its own here, as WinUI's flyouts, menus and tooltips wear acrylic
    /// (<https://learn.microsoft.com/windows/apps/design/style/acrylic>) and macOS's menus and
    /// popovers vibrancy, one material for every float. Refused: a helper in each app, which a new
    /// floating slot would miss with no error.
    ///
    /// # Panics
    ///
    /// Unless the filter holds its kinds, as a [`BackdropFilter`](crate::BackdropFilter) checks
    /// where it enters, and each float takes the material, as [`FloatMaterial::dress`] requires.
    #[track_caller]
    pub fn dress_floats(&mut self, material: FloatMaterial) {
        material.filter.validate();
        let shortcut = self.menu.item.shortcut;
        let channel = self.color_picker.label.apply(&self.text).color;
        let tooltip_ink = self.tooltip.text.apply(&self.text).color;
        let ambient = self.text.color;
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
            segmented: _,
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
            window_bg: _,
            panel_chrome: _,
            panel_clip: _,
        } = self;
        for (panel, ink) in [
            (&mut menu.panel, shortcut),
            (&mut color_picker.popup.panel, channel),
            (&mut popup.panel, shortcut),
            (&mut tooltip.panel, tooltip_ink),
            (&mut modal.panel, ambient),
        ] {
            *panel = material.dress(panel, ink);
        }
    }

    /// Whether every text slot, its font sizes scaled by `factor`, makes a face the shaper accepts:
    /// a whole style alone, an override as the face it makes over the scaled [`Self::text`]. An
    /// override valid alone can make an invalid face, as a leading factor of 1e-6 over a 14 px
    /// ambient makes a 1.4e-5 px line.
    pub(crate) fn faces_valid(&self, factor: f32) -> bool {
        let scale = |px: f32| px * factor;
        let ambient = self.text.with_font_size(scale(self.text.font_size));
        let mut valid = true;
        self.each_text(&mut |text| {
            valid &= match text {
                ThemeTextRef::Style(style) => {
                    style.with_font_size(scale(style.font_size)).metrics_valid()
                }
                ThemeTextRef::Overrides(o) => TextStyleOverrides {
                    font_size: o.font_size.map(scale),
                    ..*o
                }
                .apply(&ambient)
                .metrics_valid(),
            };
        });
        valid
    }

    /// Give each end of a lerp the text axes only the other end's overrides name, from its own
    /// [`Self::text`]: an absent axis shows the ambient value, so the lerp blends from what each
    /// end shows, as an absent value blends through its neutral. What either end shows is
    /// unchanged. The derived `lerp` runs it.
    pub(crate) fn share_text_axes(a: &mut Self, b: &mut Self) {
        let (ambient_a, ambient_b) = (a.text, b.text);
        a.for_each_text(Some(b), &mut |a, b| match (a, b) {
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
        let theme = Self {
            button: ButtonTheme::from_parts(p, m),
            checkbox: ToggleTheme::from_parts(ToggleKind::Checkbox, p, m),
            radio: ToggleTheme::from_parts(ToggleKind::Radio, p, m),
            segmented: SegmentedTheme::from_parts(p, m),
            switch: ToggleTheme::from_parts(ToggleKind::Switch, p, m),
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
            text: TextStyle::from_parts(p, m),
            window_bg: p.window_bg,
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

/// [`Theme`] as a file holds it, before the check of the whole theme: each slot checks itself
/// as it reads, and [`Theme::faces_valid`] then checks each override as the face it makes over
/// [`Theme::text`], which no slot alone can see.
#[derive(Debug, ::serde::Deserialize)]
struct UncheckedTheme {
    button: ButtonTheme,
    checkbox: ToggleTheme,
    radio: ToggleTheme,
    segmented: SegmentedTheme,
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
    window_bg: RgbaF32,
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
            segmented,
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
            window_bg,
            panel_chrome,
            panel_clip,
        } = unchecked;
        let theme = Self {
            button,
            checkbox,
            radio,
            segmented,
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
            window_bg,
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
