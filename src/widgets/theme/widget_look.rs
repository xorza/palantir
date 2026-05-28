use crate::animation::{AnimSlot, AnimSpec};
use crate::input::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::theme::text_style::TextStyle;
use palantir_anim_derive::Animatable;

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
    /// `ui.theme.text` (TextStyle is `Copy`).
    pub fn animate(
        self,
        ui: &mut Ui,
        id: WidgetId,
        fallback_text: TextStyle,
        spec: Option<AnimSpec>,
    ) -> AnimatedLook {
        let target = AnimatedLook {
            background: self.background.unwrap_or_default(),
            text: self.text.unwrap_or(fallback_text),
        };
        ui.animate(id, Self::SLOT_LOOK, target, spec)
    }
}

/// Four-state look pack reused by widgets that share Button's
/// `normal/hovered/pressed/disabled` rhythm but don't carry Button's
/// container ergonomics (`padding`/`margin`/`anim` on the outer
/// theme). [`crate::ToggleTheme`] keeps one of these per checked-state.
// **Not `Copy`** because `WidgetLook` isn't.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StatefulLook {
    pub normal: WidgetLook,
    pub hovered: WidgetLook,
    pub pressed: WidgetLook,
    pub disabled: WidgetLook,
}

/// Four-state pick precedence shared by [`StatefulLook::pick`] and
/// [`crate::widgets::theme::button::ButtonTheme::pick`] (which keeps its
/// looks as flat fields for ergonomic theme construction rather than
/// embedding a `StatefulLook`): disabled > pressed > hovered > normal.
pub(crate) fn pick_4<'a>(
    state: ResponseState,
    normal: &'a WidgetLook,
    hovered: &'a WidgetLook,
    pressed: &'a WidgetLook,
    disabled: &'a WidgetLook,
) -> &'a WidgetLook {
    if state.disabled {
        disabled
    } else if state.pressed {
        pressed
    } else if state.hovered {
        hovered
    } else {
        normal
    }
}

impl StatefulLook {
    /// Same precedence as `ButtonTheme::pick`: disabled > pressed >
    /// hovered > normal.
    pub fn pick(&self, state: ResponseState) -> &WidgetLook {
        pick_4(
            state,
            &self.normal,
            &self.hovered,
            &self.pressed,
            &self.disabled,
        )
    }
}
