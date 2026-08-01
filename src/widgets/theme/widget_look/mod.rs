//! The per-state look a themed widget paints with: [`WidgetLook`]
//! as authored, [`animated_look::AnimatedLook`] as resolved, and
//! [`stateful_look::StatefulLook`] as the four-state pack a theme
//! bundle stores.

pub(crate) mod animated_look;
pub(crate) mod stateful_look;

use crate::animation::{AnimSlot, AnimSpec};
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::animated_look::AnimatedLook;

/// Paint settings for one widget state — the same shape that Button
/// (`normal`/`hovered`/`pressed`/`disabled`) and TextEdit
/// (`normal`/`focused`/`disabled`) both reach for. `Some(x)`
/// overrides; `None` inherits the framework default for that field.
/// `background = None` inherits [`Background::default`] (paints
/// nothing — `Ui::add_shape` filters no-op shapes). `text = None`
/// inherits [`crate::Theme::text`], so an app changing
/// `theme.text.color` moves every label that didn't override it.
///
/// Per-theme `pick(state)` returns `&WidgetLook`; widgets clone the selected
/// look, then call [`Self::animate`] to interpolate its components and get an
/// [`AnimatedLook`] ready to render with.
// **Not `Copy`** because `Background` isn't — `WidgetLook` shows up in
// theme definitions and is cheap to `.clone()` (one branch for each
// `Option` + the underlying field clones).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidgetLook {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

impl WidgetLook {
    /// Slot [`Self::animate`] reserves on the widget's id. One row
    /// per widget animates the whole resolved look (background + text)
    /// — halves `Ui::animate` call traffic compared to per-component
    /// slots.
    const SLOT_LOOK: AnimSlot = AnimSlot::new("look");

    /// Resolve the look to flat animated values. `Background` (fill +
    /// stroke) animates as one slot; `TextStyle` (color animated,
    /// font/leading snapped) as another. Pass `spec = None` to snap
    /// everything; call shape stays the same so callers don't fork
    /// on motion.
    ///
    /// `fallback_text` is used when `self.text == None`. The selected look is
    /// consumed so its background moves into the animation target.
    #[inline(always)]
    pub fn animate(
        self,
        ui: &mut Ui,
        id: WidgetId,
        fallback_text: &TextStyle,
        spec: Option<AnimSpec>,
    ) -> AnimatedLook {
        let target = AnimatedLook {
            background: self.background.unwrap_or_default(),
            text: self.text.unwrap_or_else(|| fallback_text.clone()),
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
