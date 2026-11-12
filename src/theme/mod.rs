//! The theme bundle every widget styles from: one submodule per widget theme, over the shared [`palette`], [`text_style`], [`text_style_overrides`] and [`look`] vocabulary.
//!
//! [`Theme`] aggregates them; a widget reads only its own slice, so a bundle grows a field without changing existing widgets.
//!
//! Plain look data: it knows no `Widget` or `Ui`, and `Widget::wear` applies a look.

pub(crate) mod button;
pub(crate) mod check_state;
pub(crate) mod close_shown;
pub(crate) mod color_picker;
pub(crate) mod color_scheme;
pub(crate) mod combo_box;
pub(crate) mod dock;
pub(crate) mod drag_value;
mod duration_codec;
pub(crate) mod error;
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
use crate::effect::press_input::PressInput;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::{Material, fallback_alpha};
use crate::theme::button::ButtonTheme;
use crate::theme::close_shown::CloseShown;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::combo_box::ComboBoxTheme;
use crate::theme::dock::DockTheme;
use crate::theme::drag_value::DragValueTheme;
use crate::theme::expander::ExpanderTheme;
use crate::theme::focus_ring::FocusRingTheme;
use crate::theme::look::press_effect::PressEffect;
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
use ::serde::de::Error as _;
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
    Clone, Debug, PartialEq, ::serde::Serialize, palantir_derive::Lerp, palantir_derive::ThemeWalk,
)]
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
    /// What the box of a [`Material`] that names no fallback of its own paints in place of its
    /// fill while transparency is reduced
    /// ([`MaterialFallback::ThemeSurface`](crate::MaterialFallback::ThemeSurface)): in a theme from
    /// parts the popup panel's fill at the least alpha that holds [`Palette::text`] at 4.5:1 over
    /// any page, opaque where none does, so the box reads as the theme's float surface,
    /// translucent and readable.
    pub material_fallback: RgbaF32,
    /// The spec a change to this theme runs under, whether [`Ui::set_theme`](crate::Ui::set_theme)
    /// or a turn of the scheme brought it, as CSS reads the new values of `transition-*`
    /// (<https://www.w3.org/TR/css-transitions-1/>). [`AnimationSpec::THEME`] in a stock theme. A
    /// blend holds one end's, so read it from [`Ui::scheme_theme`](crate::Ui::scheme_theme).
    #[animate(snap)]
    pub transition: AnimationSpec,
    /// Default chrome for in-tree containers (`Panel`, `Grid`) without chrome of their own. `None` leaves them unpainted; a popup reads [`Self::popup`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_chrome: Option<Chrome>,
    /// Default clip mode for containers that set none; the rounded mask uses [`Self::panel_chrome`]'s `radius`.
    #[serde(skip_serializing_if = "is_clip_none")]
    #[animate(snap)]
    pub panel_clip: ClipMode,
}

/// One text-bearing slot of a theme, as [`ThemeWalk::for_each_text`] hands it over: a whole style, or a look's overrides folded onto [`Theme::text`].
#[derive(Debug)]
pub(crate) enum ThemeText<'a> {
    Style(&'a mut TextStyle),
    Overrides(&'a mut TextStyleOverrides),
}

/// [`ThemeText`] to read, as [`ThemeWalk::each_text`] hands it over.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ThemeTextRef<'a> {
    Style(&'a TextStyle),
    Overrides(&'a TextStyleOverrides),
}

/// A theme part's text, press and material slots: [`Self::for_each_text`] visits each text slot,
/// beside the same slot of `other` when one is given, [`Self::each_text`] reads them, and
/// [`Self::each_press`] and [`Self::each_material`] each press and material slot, in field order.
/// [`Theme::faces_valid`], [`Theme::scale_text`], [`Theme::share_text_axes`],
/// [`Theme::press_effects`] and [`Theme::materials`] drive it. A part derives it and walks every
/// field, so each field's type implements it, a value that holds none of the slots through
/// `no_theme_walk!`, and a field of a new type fails to compile until its type says what it holds.
pub(crate) trait ThemeWalk {
    /// Each text slot, in field order, to read.
    fn each_text<F>(&self, f: &mut F)
    where
        F: FnMut(ThemeTextRef<'_>);

    fn for_each_text<F>(&mut self, other: Option<&mut Self>, f: &mut F)
    where
        F: FnMut(ThemeText<'_>, Option<ThemeText<'_>>);

    /// Each press slot, in field order, to read ([`Theme::press_effects`]); a value that holds
    /// none visits nothing.
    #[inline]
    fn each_press<F>(&self, _: &mut F)
    where
        F: FnMut(&PressEffect),
    {
    }

    /// Each material a slot holds, in field order, to read ([`Theme::materials`]); a value that
    /// holds none visits nothing.
    #[inline]
    fn each_material<F>(&self, _: &mut F)
    where
        F: FnMut(&Material),
    {
    }
}

/// A walk that visits nothing, for each value type that holds no text or press slot.
macro_rules! no_theme_walk {
    ($($ty:ty),* $(,)?) => {$(
        impl ThemeWalk for $ty {
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

no_theme_walk!(
    f32,
    Vec2,
    [Vec2; 3],
    Size,
    Spacing,
    Duration,
    RgbaF32,
    AnimationSpec,
    ClipMode,
    CloseShown,
    Chrome,
    Option<Chrome>,
);

/// A whole style.
impl ThemeWalk for TextStyle {
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

/// A material a slot may leave out, which holds no text.
impl ThemeWalk for Option<Material> {
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

    #[inline]
    fn each_material<F>(&self, f: &mut F)
    where
        F: FnMut(&Material),
    {
        if let Some(material) = self {
            f(material);
        }
    }
}

/// A press slot, which holds no text.
impl ThemeWalk for Option<PressEffect> {
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

    #[inline]
    fn each_press<F>(&self, f: &mut F)
    where
        F: FnMut(&PressEffect),
    {
        if let Some(press) = self {
            f(press);
        }
    }
}

/// A look's overrides, folded onto [`Theme::text`].
impl ThemeWalk for TextStyleOverrides {
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
    /// fill's color at the material's fill alpha over its material, blurred, frosted or glass, held so the
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
    /// Unless the material holds its kinds, as a [`Material`] checks where it
    /// enters, and each float takes the material, as [`FloatMaterial::dress`] requires.
    #[track_caller]
    pub fn dress_floats(&mut self, material: &FloatMaterial) {
        material.validate();
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
            material_fallback: _,
            transition: _,
            panel_chrome: _,
            panel_clip: _,
        } = self;
        for (panel, worn, ink) in [
            (&mut menu.panel, &mut menu.material, shortcut),
            (
                &mut color_picker.popup.panel,
                &mut color_picker.popup.material,
                channel,
            ),
            (&mut popup.panel, &mut popup.material, shortcut),
            (&mut tooltip.panel, &mut tooltip.material, tooltip_ink),
            (&mut modal.panel, &mut modal.panel_material, ambient),
        ] {
            material.dress(panel, worn, ink);
        }
    }

    /// The effect each press slot names, in field order, a name once for each slot that names it.
    pub(crate) fn press_effects(&self, mut f: impl FnMut(EffectName<PressInput>)) {
        self.each_press(&mut |press: &PressEffect| f(press.effect));
    }

    /// Each slot's material, in field order.
    pub(crate) fn materials(&self, mut f: impl FnMut(&Material)) {
        self.each_material(&mut f);
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
            material_fallback: material_fallback(p, m),
            transition: AnimationSpec::THEME,
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

/// The popup panel's fill at the least alpha that holds the palette's text at 4.5:1 over any page,
/// opaque where none does.
fn material_fallback(p: &Palette, m: &Metrics) -> RgbaF32 {
    let surface = p
        .popup_panel(m)
        .paint
        .fill
        .as_solid()
        .expect("the popup panel's fill is a color");
    surface.with_alpha(fallback_alpha(p.text, surface))
}

/// [`Theme`] as a file holds it, read straight into a `Theme` by serde's remote derive
/// (<https://serde.rs/remote-derive.html>), so the fields are listed once more and the compiler
/// holds the list whole. Each slot checks itself as it reads, and [`Theme`]'s `Deserialize` then
/// checks each override as the face it makes over [`Theme::text`] ([`Theme::faces_valid`]),
/// which no slot alone can see. Refused: `#[serde(remote = "Self")]` on `Theme`, whose derive adds
/// a public inherent `Theme::deserialize` that skips the check, and that the call
/// `Theme::deserialize(d)` picks over the trait's.
#[derive(Debug, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(remote = "Theme")]
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
    material_fallback: RgbaF32,
    transition: AnimationSpec,
    // This and the next are left out of a file when absent, as `Theme` writes them.
    #[serde(default)]
    panel_chrome: Option<Chrome>,
    #[serde(default)]
    panel_clip: ClipMode,
}

impl<'de> ::serde::Deserialize<'de> for Theme {
    fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let theme = UncheckedTheme::deserialize(deserializer)?;
        if !theme.faces_valid(1.0) {
            return Err(D::Error::custom(FACES_ERROR));
        }
        Ok(theme)
    }
}

#[cfg(test)]
mod tests;
