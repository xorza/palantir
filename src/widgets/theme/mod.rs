pub(crate) mod button;
pub(crate) mod context_menu;
pub(crate) mod drag_value;
pub(crate) mod modal;
pub(crate) mod palette;
pub(crate) mod progress_bar;
pub(crate) mod scrollbar;
pub(crate) mod separator;
pub(crate) mod serde;
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
use crate::text::text_metrics_valid;
use crate::ui::Ui;
use crate::widgets::theme::button::ButtonTheme;
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
use crate::widgets::theme::widget_look::{AnimatedLook, WidgetLook};
/// Global theme. Aggregates per-widget themes. Widgets opt in by reading
/// from `Ui::theme`.
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
    pub checkbox: ToggleTheme,
    pub radio: ToggleTheme,
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
            metrics_valid &= text_metrics_valid(font_size_px, style.line_height_for(font_size_px));
        });
        assert!(metrics_valid, "{SCALED_TEXT_METRICS_ERROR}");
        self.for_each_text(|t| t.font_size_px *= ratio);
        self.text_scale = scale;
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
        self.tooltip.for_each_text(f);
    }
}

/// The shape a per-widget theme bundle needs for [`resolve_look`]:
/// a per-state [`WidgetLook`] pick plus the box defaults
/// (padding / margin / motion) that fill in fields the builder did not
/// configure. Implemented by [`ButtonTheme`] and [`TextEditTheme`]; each
/// impl defines its own `active` semantics by delegating to its inherent
/// `pick`.
pub(crate) trait WidgetTheme {
    fn pick(&self, state: &ResponseState) -> &WidgetLook;
    fn padding(&self) -> Spacing;
    fn margin(&self) -> Spacing;
    fn anim(&self) -> Option<AnimSpec>;
}

/// Resolve a widget's animated look from its theme: pick the per-state
/// [`WidgetLook`], fill in padding/margin the caller did not configure,
/// and animate. Used by every
/// chrome-box widget (`Button` / `ComboBox` / `DragValue` / `TextEdit`).
/// The scalars are copied out so the borrow on `ui.theme` (borrowed,
/// not cloned) ends before `animate` reborrows `ui` mutably. `style` of
/// `None` inherits `fallback(&ui.theme)` — the widget's own global
/// theme slot (`theme.button` for Button/ComboBox,
/// `theme.drag_value.chip` for the DragValue chip, `theme.text_edit`
/// for TextEdit).
// This generic crosses the theme/widget codegen-unit boundary. Leaving it to
// the default inliner kept the resolver plus its tiny trait accessors outlined
// in release builds; the frame bench measured that path at 3.9% precise
// self-time. Force the whole lookup chain into each widget so state picking,
// default resolution and target construction optimize as one block.
#[inline(always)]
pub(crate) fn resolve_look<T: WidgetTheme>(
    ui: &mut Ui,
    id: WidgetId,
    node: &mut Node,
    state: &ResponseState,
    style: Option<&T>,
    fallback: impl FnOnce(&Theme) -> &T,
) -> AnimatedLook {
    let fallback_text = ui.theme.text.clone();
    let style = style.unwrap_or_else(|| fallback(&ui.theme));
    let padding = style.padding();
    let margin = style.margin();
    let anim = style.anim();
    let look_target = style.pick(state).clone();
    node.padding.get_or_insert(padding);
    node.margin.get_or_insert(margin);
    look_target.animate(ui, id, &fallback_text, anim)
}

impl Theme {
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

impl Default for Theme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
