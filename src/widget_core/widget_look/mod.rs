//! The per-state look a themed widget paints with, in the four shapes it
//! passes through:
//!
//! - [`WidgetLook`] — one state, as authored in a theme file.
//! - [`stateful_look::StatefulLook`] — the four-state pack a theme bundle
//!   stores, and the `normal` / `hovered` / `active` / `disabled` precedence
//!   every widget picks from.
//! - [`animated_look::AnimatedLook`] — one state with its text overrides
//!   folded onto the ambient style, which is what `Ui::animate`
//!   interpolates.
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
use crate::primitives::paint::background::Background;
use crate::widget_core::widget_look::animated_look::AnimatedLook;
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::text_style::{TextStyle, TextStyleOverrides};

/// Paint settings for one widget state — the same shape every
/// state-styled widget reaches for, four to a
/// [`StatefulLook`](stateful_look::StatefulLook). The engaged state is
/// `active` on all of them: pressed for Button, focused for TextEdit.
///
/// `text` overrides [`crate::Theme::text`] axis by axis, so a look that
/// dims the ink names the colour alone and keeps the theme's size and
/// face: an app changing `theme.text.font_size_px` moves every label,
/// and one changing `theme.text.color` moves every label whose look
/// didn't name a colour. `background` has no ambient to inherit —
/// [`Background::NONE`] already *is* "paints nothing", and
/// `Ui::add_shape` filters no-op chrome.
///
/// Per-theme `pick(state)` returns `&WidgetLook`; [`Self::to_animated`]
/// folds the text overrides onto the ambient style into the
/// [`AnimatedLook`] target `Ui::animate` interpolates toward.
// **Not `Copy`** because `Background` isn't — `WidgetLook` shows up in
// theme definitions and is cheap to `.clone()`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidgetLook {
    /// Fill, stroke, corners and shadow. [`Background::NONE`] paints
    /// nothing.
    pub background: Background,
    /// Text axes this look sets over [`Theme::text`](crate::Theme).
    /// [`TextStyleOverrides::NONE`] inherits every one.
    #[serde(default, skip_serializing_if = "TextStyleOverrides::is_empty")]
    pub text: TextStyleOverrides,
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
    /// The overrides fold onto `ambient_text` here, before the tween, so
    /// the tween runs between two whole styles and a colour-only look
    /// cross-fades its colour against the ambient one.
    ///
    /// Separate from the `Ui::animate` call that consumes the result
    /// because the caller reads `ambient_text` out of `ui.theme`, and
    /// that borrow has to end before `ui` is reborrowed mutably.
    #[inline(always)]
    pub fn to_animated(&self, ambient_text: TextStyle) -> AnimatedLook {
        AnimatedLook {
            background: self.background.clone(),
            text: self.text.apply(&ambient_text),
        }
    }

    /// Visit this look's text overrides.
    ///
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(crate) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            text,
            background: _,
        } = self;
        f(ThemeText::Overrides(text));
    }
}
