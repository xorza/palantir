/// `Default` for a theme bundle whose default *is* the default palette.
///
///
/// Every bundle in here builds from a [`palette::Palette`], and the
/// stock look is that recipe over [`palette::Palette::DEFAULT`] — so the
/// impl is the same line each time and only the type varies. Invoke it
/// **in the bundle's own file**, next to `from_palette`.
macro_rules! palette_default {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl Default for $ty {
                fn default() -> Self {
                    Self::from_palette(&$crate::widgets::theme::palette::Palette::DEFAULT)
                }
            }
        )+
    };
}

/// Implement [`WidgetTheme`] for a bundle that stores its box defaults
/// in fields called `padding` / `margin` / `anim` and its per-state pick
/// in an inherent `pick`.
///
/// The three accessors forward to identically-named fields on every
/// implementor, so only `Mode` and the shape of `pick` actually vary:
///
/// - `impl_widget_theme!(ButtonTheme)` — `Mode = ()`, the engaged state
///   falls out of the response, so `pick` takes only the state.
/// - `impl_widget_theme!(ToggleTheme, mode: bool)` — the mode reaches
///   `pick` as a second argument, because a toggle's look pack is chosen
///   by a flag the response can't answer.
///
/// Invoke it **in the bundle's own file**, next to its inherent `pick`.
macro_rules! impl_widget_theme {
    ($ty:ty) => {
        impl $crate::widgets::theme::WidgetTheme for $ty {
            type Mode = ();
            #[inline(always)]
            fn pick(
                &self,
                state: &$crate::input::response::ResponseState,
                (): (),
            ) -> &$crate::widgets::theme::widget_look::WidgetLook {
                <$ty>::pick(self, state)
            }
            impl_widget_theme!(@box_defaults);
        }
    };
    ($ty:ty, mode: $mode:ty) => {
        impl $crate::widgets::theme::WidgetTheme for $ty {
            type Mode = $mode;
            #[inline(always)]
            fn pick(
                &self,
                state: &$crate::input::response::ResponseState,
                mode: $mode,
            ) -> &$crate::widgets::theme::widget_look::WidgetLook {
                // Path form, not `self.pick(…)`: the inherent method and
                // this one have the same arity here, so the receiver form
                // would read as though it might recurse.
                <$ty>::pick(self, state, mode)
            }
            impl_widget_theme!(@box_defaults);
        }
    };
    (@box_defaults) => {
        #[inline(always)]
        fn padding(&self) -> $crate::primitives::spacing::Spacing {
            self.padding
        }
        #[inline(always)]
        fn margin(&self) -> $crate::primitives::spacing::Spacing {
            self.margin
        }
        #[inline(always)]
        fn anim(&self) -> Option<$crate::animation::AnimSpec> {
            self.anim
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

#[cfg(test)]
mod tests;

use crate::animation::AnimSpec;
use crate::input::response::ResponseState;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::text::key;
use crate::ui::Ui;
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
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::animated_look::AnimatedLook;
/// Global theme. Aggregates per-widget themes. Widgets opt in by reading
/// from `Ui::theme`.
///
/// # Overriding a widget's look
///
/// Every themed widget takes `.style(&XTheme)`, which replaces its whole
/// bundle for that call. It is all-or-nothing by design — to move one
/// axis, build the bundle from the theme:
/// `SpinnerTheme { color: red, ..ui.theme.spinner.clone() }`.
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
    /// Theme slot for `Button`s used as menu-bar triggers — flat,
    /// hover-on-only, opens a popup on click. Distinct from `button`
    /// so apps can restyle one without affecting in-flow buttons,
    /// and from `context_menu.item` which is for *rows inside* the
    /// popup. Default built by [`ButtonTheme::menu_button`].
    pub menu_button: ButtonTheme,
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
    /// override the axis. Not a per-widget theme: `Button` and `TextEdit`
    /// carry their own state-dependent colours.
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
    #[serde(
        default = "default_text_scale",
        deserialize_with = "crate::widgets::theme::serde::deserialize_text_scale"
    )]
    text_scale: f32,
}

const TEXT_SCALE_ERROR: &str = "text scale must be finite and positive";
const SCALED_TEXT_METRICS_ERROR: &str = "text scale would make font size or line height invalid";

#[inline]
fn is_clip_none(c: &ClipMode) -> bool {
    matches!(c, ClipMode::None)
}

#[inline]
fn default_text_scale() -> f32 {
    1.0
}

#[inline]
fn text_scale_is_valid(scale: f32) -> bool {
    scale.is_finite() && scale > 0.0
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
        assert!(text_scale_is_valid(scale), "{TEXT_SCALE_ERROR}");
        let ratio = scale / self.text_scale;
        let mut metrics_valid = true;
        self.for_each_text(|style| {
            let font_size_px = style.font_size_px * ratio;
            metrics_valid &=
                key::text_metrics_valid(font_size_px, style.line_height_for(font_size_px));
        });
        assert!(metrics_valid, "{SCALED_TEXT_METRICS_ERROR}");
        self.for_each_text(|t| t.font_size_px *= ratio);
        self.text_scale = scale;
    }

    /// Visit every `TextStyle` in the theme. `set_text_scale` drives the
    /// walk; each sub-theme owns its own visit (see each `for_each_text`).
    ///
    /// **Every `for_each_text` in this module destructures its whole
    /// struct**, binding the text-free fields to `_`, so a new field
    /// anywhere in the theme tree fails to compile here until someone
    /// classifies it as text-bearing or not. That is the guarantee; the
    /// runtime backstop is
    /// `tests::text_scale::set_text_scale_reaches_every_font_size`,
    /// which scales a default theme and asserts over its serialized
    /// form that every `font_size_px` moved. The test can only see
    /// styles the default theme materializes — an `Option<TextStyle>`
    /// left `None` by default is invisible to it — which is exactly the
    /// gap the destructuring closes.
    fn for_each_text(&mut self, mut f: impl FnMut(&mut TextStyle)) {
        let Self {
            text,
            button,
            menu_button,
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
            text_scale: _,
        } = self;
        let f = &mut f;
        f(text);
        button.for_each_text(f);
        menu_button.for_each_text(f);
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
            menu_button: ButtonTheme::menu_button(p),
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
            text_scale: default_text_scale(),
        }
    }
}

/// A per-widget theme bundle that can resolve into a painted look: a
/// per-state [`WidgetLook`] pick plus the box defaults (padding /
/// margin / motion) that fill in fields the builder did not configure.
///
/// Every widget that paints state-dependent chrome implements it —
/// [`ButtonTheme`], [`TextEditTheme`], `MenuItemTheme`, [`ToggleTheme`]
/// — so [`Self::resolve`] is the one path from a theme bundle to a
/// rendered look, and a widget cannot quietly grow a fifth. Each impl
/// defines its own `active` semantics by delegating to its inherent
/// `pick`; `impl_widget_theme!` writes the forwarding for all four.
pub(super) trait WidgetTheme: Sized {
    /// Pick input the [`ResponseState`] can't supply.
    ///
    /// `()` wherever the engaged state falls out of the response alone
    /// (pressed for `Button` / `MenuItem`, focused for `TextEdit`);
    /// `bool` for [`ToggleTheme`], whose checked flag chooses *which of
    /// its two look packs* the four-state pick then runs inside.
    type Mode: Copy;

    fn pick(&self, state: &ResponseState, mode: Self::Mode) -> &WidgetLook;
    fn padding(&self) -> Spacing;
    fn margin(&self) -> Spacing;
    fn anim(&self) -> Option<AnimSpec>;

    /// Resolve a widget's animated look: pick the per-state
    /// [`WidgetLook`], fill in padding/margin the caller did not
    /// configure, and animate. **The only route from a theme bundle to
    /// a painted look** — `Button`, `ComboBox`, `DragValue`'s chip,
    /// `TextEdit`, `MenuItem`, and the three toggles (through
    /// `toggle::toggle_row`) all arrive here, so per-state precedence,
    /// spacing defaults, and transitions are one behaviour rather than
    /// one per widget.
    ///
    /// An associated function rather than a method: `style` is an
    /// `Option`, and the `None` case is the whole point — it inherits
    /// `fallback(&ui.theme)`, the widget's own global slot
    /// (`theme.button` for Button/ComboBox, `theme.drag_value.chip` for
    /// the DragValue chip, `theme.text_edit` for TextEdit,
    /// `theme.context_menu.item` for MenuItem, and one of
    /// `theme.checkbox` / `theme.radio` / `theme.switch` for the
    /// toggles, which share a theme *type* but not a *slot*). `Self` is
    /// inferred from `style`, so call sites read
    /// `WidgetTheme::resolve(ui, …)` with no turbofish.
    ///
    /// The scalars are copied out, and the look is flattened into an
    /// owned target, so every borrow on `ui.theme` ends before
    /// [`Ui::animate`] reborrows `ui` mutably. That split is what lets
    /// [`Theme::text`] be passed by reference: it is copied only into
    /// the target of a look that declines to override it, rather than
    /// cloned for every themed widget to launder the borrow.
    // This generic crosses the theme/widget codegen-unit boundary. Leaving it
    // to the default inliner kept the resolver plus its tiny trait accessors
    // outlined in release builds; the frame bench measured that path at 3.9%
    // precise self-time. Force the whole lookup chain into each widget so state
    // picking, default resolution and target construction optimize as one block.
    #[inline(always)]
    fn resolve(
        ui: &mut Ui,
        id: WidgetId,
        node: &mut Node,
        state: &ResponseState,
        mode: Self::Mode,
        style: Option<&Self>,
        fallback: impl FnOnce(&Theme) -> &Self,
    ) -> AnimatedLook {
        let style = style.unwrap_or_else(|| fallback(&ui.theme));
        let padding = style.padding();
        let margin = style.margin();
        let anim = style.anim();
        let target = style.pick(state, mode).to_animated(&ui.theme.text);
        node.padding.get_or_insert(padding);
        node.margin.get_or_insert(margin);
        ui.animate(id, WidgetLook::SLOT_LOOK, target, anim)
    }
}

palette_default!(Theme);
