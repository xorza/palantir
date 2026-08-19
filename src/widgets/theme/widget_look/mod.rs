//! The per-state look a themed widget paints with: [`WidgetLook`]
//! as authored, [`animated_look::AnimatedLook`] as resolved,
//! [`stateful_look::StatefulLook`] as the four-state pack a theme
//! bundle stores, and [`look_plan::LookPlan`] as the owned hand-off
//! between reading the theme and animating toward what it said.

pub(crate) mod animated_look;
pub(crate) mod look_plan;
pub(crate) mod stateful_look;

use crate::animation::AnimSlot;
use crate::primitives::background::Background;
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
/// Per-theme `pick(state)` returns `&WidgetLook`; [`Self::to_animated`]
/// flattens the selected look into the [`AnimatedLook`] target
/// `Ui::animate` interpolates toward.
// **Not `Copy`** because `Background` isn't — `WidgetLook` shows up in
// theme definitions and is cheap to `.clone()` (one branch for each
// `Option` + the underlying field clones).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidgetLook {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

impl WidgetLook {
    /// Slot the resolved look reserves on the widget's id. One row
    /// per widget animates the whole look (background + text) — halves
    /// `Ui::animate` call traffic compared to per-component slots.
    pub(crate) const SLOT_LOOK: AnimSlot = AnimSlot::new("look");

    /// Flatten the look into the target `Ui::animate` interpolates
    /// toward: `Background` (fill + stroke) animates, `TextStyle`
    /// carries its animated colour and snapped font/leading.
    ///
    /// `fallback_text` is read only when `self.text` is `None`, so a
    /// look that overrides text never copies [`Theme::text`](crate::Theme).
    /// It stays a reference — and this stays separate from the
    /// `Ui::animate` call that consumes the result — because the caller
    /// borrows it straight out of `ui.theme`, and that borrow has to end
    /// before `ui` is reborrowed mutably. Folding the two together is
    /// what forced the old unconditional clone.
    #[inline(always)]
    pub fn to_animated(&self, fallback_text: &TextStyle) -> AnimatedLook {
        AnimatedLook {
            background: self.background.clone().unwrap_or_default(),
            text: self.text.clone().unwrap_or_else(|| fallback_text.clone()),
        }
    }

    /// Visit this look's overriding `TextStyle`, if any. An unset look
    /// inherits `Theme::text` (visited separately), so it carries none.
    ///
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self {
            text,
            background: _,
        } = self;
        if let Some(t) = text {
            f(t);
        }
    }
}
