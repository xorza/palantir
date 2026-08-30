//! The per-state look a themed widget paints with, in the four shapes it
//! passes through:
//!
//! - [`WidgetLook`] — one state, as authored in a theme file.
//! - [`stateful_look::StatefulLook`] — the four-state pack a theme bundle
//!   stores, and the `normal` / `hovered` / `active` / `disabled` precedence
//!   every widget picks from.
//! - [`animated_look::AnimatedLook`] — one state with the ambient text
//!   fallback resolved, which is what `Ui::animate` interpolates.
//! - [`look_plan::LookPlan`] — that target plus the bundle's box defaults,
//!   owned, so the theme borrow can end before the `Ui` is reborrowed.
//!
//! [`theme_slot::ThemeSlot`] spans the last two: a bundle names its pick and
//! its [`theme_slot::SlotDefaults`] once, and every widget reaches a
//! `LookPlan` through that one call.

pub(crate) mod animated_look;
pub(crate) mod look_plan;
pub(crate) mod stateful_look;
pub(crate) mod theme_slot;

use crate::animation::anim_slot::AnimSlot;
use crate::primitives::background::Background;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::animated_look::AnimatedLook;

/// Paint settings for one widget state — the same shape every
/// state-styled widget reaches for, four to a
/// [`StatefulLook`](stateful_look::StatefulLook). The engaged state is
/// `active` on all of them: pressed for Button, focused for TextEdit.
///
/// `text` is the one optional axis: `None` inherits
/// [`crate::Theme::text`], so an app changing `theme.text.color` moves
/// every label that didn't override it. `background` has no such ambient
/// to inherit — [`Background::NONE`] already *is* "paints nothing", and
/// `Ui::add_shape` filters no-op chrome — so it is a plain value rather
/// than an `Option` whose empty case would mean the same thing.
///
/// Per-theme `pick(state)` returns `&WidgetLook`; [`Self::to_animated`]
/// resolves the text fallback into the [`AnimatedLook`] target
/// `Ui::animate` interpolates toward.
// **Not `Copy`** because `Background` isn't — `WidgetLook` shows up in
// theme definitions and is cheap to `.clone()`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidgetLook {
    pub background: Background,
    pub text: Option<TextStyle>,
}

impl WidgetLook {
    /// Slot the resolved look reserves on the widget's id. One row
    /// per widget animates the whole look (background + text) — halves
    /// `Ui::animate` call traffic compared to per-component slots.
    pub(crate) const SLOT_LOOK: AnimSlot = AnimSlot::new("look");

    /// Resolve the look into the target `Ui::animate` interpolates
    /// toward: `Background` (fill + stroke) animates, `TextStyle`
    /// carries its animated colour and snapped font/leading.
    ///
    /// `fallback_text` is read only when `self.text` is `None`, so a
    /// look that overrides text never copies [`Theme::text`](crate::Theme).
    /// It stays a reference — and this stays separate from the
    /// `Ui::animate` call that consumes the result — because the caller
    /// borrows it straight out of `ui.theme`, and that borrow has to end
    /// before `ui` is reborrowed mutably. Folding the two together would
    /// force an unconditional clone.
    #[inline(always)]
    pub fn to_animated(&self, fallback_text: &TextStyle) -> AnimatedLook {
        AnimatedLook {
            background: self.background.clone(),
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
