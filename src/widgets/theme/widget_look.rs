use crate::animation::{AnimSlot, AnimSpec};
use crate::input::response::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::theme::text_style::TextStyle;
use aperture_anim_derive::Animatable;

/// Paint settings for one widget state — the same shape that Button
/// (`normal`/`hovered`/`pressed`/`disabled`) and TextEdit
/// (`normal`/`focused`/`disabled`) both reach for. `Some(x)`
/// overrides; `None` inherits the framework default for that field.
/// `background = None` inherits [`Background::default`] (paints
/// nothing — `Ui::add_shape` filters no-op shapes). `text = None`
/// inherits [`crate::Theme::text`], so an app changing
/// `theme.text.color` moves every label that didn't override it.
///
/// Per-theme `pick(state)` returns `&WidgetLook`; widgets call
/// [`Self::animate`] to interpolate the look's components and get an
/// [`AnimatedLook`] ready to render with.
// **Not `Copy`** because `Background` isn't — `WidgetLook` shows up in
// theme definitions and is cheap to `.clone()` (one branch for each
// `Option` + the underlying field clones).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidgetLook {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

/// Resolved + per-frame animated values for a [`WidgetLook`]. Built
/// by [`WidgetLook::animate`]. Widgets read `background` and `text`
/// directly; both fields are already-animated.
///
/// `text.color` is the animated color; `text.font_size_px` and
/// `text.line_height_mult` are snap-carried from the picked
/// `WidgetLook` (or the fallback) — see `TextStyle`'s
/// `#[animate(snap)]` markings.
// **Not `Copy`** because `Background` isn't.
#[derive(Clone, Debug, Default, PartialEq, Animatable)]
pub struct AnimatedLook {
    pub background: Background,
    pub text: TextStyle,
}

impl AnimatedLook {
    /// Convenience: `text.line_height_for(text.font_size_px)`. Widgets
    /// rendering `ShapeRecord::Text` need this paired with `font_size_px`
    /// for the shaper.
    pub fn line_height_px(&self) -> f32 {
        self.text.line_height_for(self.text.font_size_px)
    }
}

impl WidgetLook {
    /// Slot [`Self::animate`] reserves on the widget's id. One row
    /// per widget animates the whole resolved look (background + text)
    /// — halves `Ui::animate` call traffic compared to per-component
    /// slots.
    const SLOT_LOOK: AnimSlot = AnimSlot("look");

    /// Resolve the look to flat animated values. `Background` (fill +
    /// stroke) animates as one slot; `TextStyle` (color animated,
    /// font/leading snapped) as another. Pass `spec = None` to snap
    /// everything; call shape stays the same so callers don't fork
    /// on motion.
    ///
    /// `fallback_text` is used when `self.text == None` — pass
    /// `ui.theme.text` (TextStyle is `Copy`). Takes `&self` so a look
    /// borrowed from an owned theme animates without a clone; callers
    /// whose look borrows `ui.theme` still clone first to end that
    /// borrow before `ui` is reborrowed mutably.
    #[inline(always)]
    pub fn animate(
        &self,
        ui: &mut Ui,
        id: WidgetId,
        fallback_text: TextStyle,
        spec: Option<AnimSpec>,
    ) -> AnimatedLook {
        let target = AnimatedLook {
            background: self.background.clone().unwrap_or_default(),
            text: self.text.unwrap_or(fallback_text),
        };
        ui.animate(id, Self::SLOT_LOOK, target, spec)
    }

    /// Visit this look's overriding `TextStyle`, if any. An unset look
    /// inherits `Theme::text` (visited separately), so it carries none.
    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        if let Some(t) = &mut self.text {
            f(t);
        }
    }
}

/// The uniform four-state look pack every state-styled widget theme
/// carries: `normal` / `hovered` / `active` / `disabled`. `active` is
/// the widget's *engaged* state — pressed for Button / Toggle /
/// MenuItem, focused for TextEdit — supplied per widget as
/// [`Self::pick`]'s flag so the precedence stays identical everywhere.
/// [`crate::ButtonTheme`], [`crate::TextEditTheme`], and
/// [`crate::MenuItemTheme`] embed one (serde-flattened);
/// [`crate::ToggleTheme`] keeps one per checked-state.
// **Not `Copy`** because `WidgetLook` isn't.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StatefulLook {
    pub normal: WidgetLook,
    pub hovered: WidgetLook,
    pub active: WidgetLook,
    pub disabled: WidgetLook,
}

impl StatefulLook {
    /// Uniform pick precedence: disabled > active > hovered > normal.
    /// `active` is the widget's engaged flag (`state.pressed()` for
    /// press-driven widgets, `state.focused` for focus-driven ones);
    /// `disabled` / `hovered` read straight from `state`.
    #[inline(always)]
    pub fn pick(&self, state: ResponseState, active: bool) -> &WidgetLook {
        if state.disabled {
            &self.disabled
        } else if active {
            &self.active
        } else if state.hovered {
            &self.hovered
        } else {
            &self.normal
        }
    }

    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        self.normal.for_each_text(f);
        self.hovered.for_each_text(f);
        self.active.for_each_text(f);
        self.disabled.for_each_text(f);
    }
}
