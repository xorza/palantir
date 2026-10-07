//! The per-state look a themed widget paints with:
//!
//! - [`WidgetLook`]: one state, as authored in a theme file.
//! - [`stateful_look::StatefulLook`]: the four-state pack a theme bundle stores.
//! - [`animated_look::AnimatedLook`]: one state with text overrides folded onto
//!   the ambient style, which `Ui::animate` interpolates.
//! - [`look_plan::LookPlan`]: that target plus box defaults, owned so the theme
//!   borrow can end.
//!
//! [`theme_slot::ThemeSlot`] names a bundle's pick and defaults once.

pub(crate) mod animated_look;
pub(crate) mod look_plan;
pub(crate) mod stateful_look;
pub(crate) mod theme_slot;

use crate::animation::animation_slot::AnimationSlot;
use crate::primitives::paint::background::Background;
use crate::widget_core::widget_look::animated_look::AnimatedLook;
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::text_style::{TextStyle, TextStyleOverrides};

/// Paint settings for one widget state, four to a
/// [`StatefulLook`](stateful_look::StatefulLook); the engaged state is `active`
/// on all (pressed for Button, focused for TextEdit). `text` overrides
/// [`crate::Theme::text`] axis by axis. [`Self::to_animated`] folds them into
/// the [`AnimatedLook`] target `Ui::animate` interpolates toward.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidgetLook {
    /// Fill, stroke, corners and shadow.
    pub background: Background,
    /// Text axes this look sets over [`Theme::text`](crate::Theme); [`TextStyleOverrides::NONE`] inherits all.
    #[serde(default, skip_serializing_if = "TextStyleOverrides::is_empty")]
    pub text: TextStyleOverrides,
}

impl WidgetLook {
    /// Slot the resolved look reserves on the widget's id.
    pub(crate) const SLOT_LOOK: AnimationSlot = AnimationSlot::new("look");

    /// Resolves the look into the target `Ui::animate` interpolates toward,
    /// folding overrides onto `ambient_text` first so a colour-only look
    /// cross-fades against the ambient colour. Separate from `Ui::animate`
    /// because the `ui.theme` borrow must end before `ui` is reborrowed.
    #[inline(always)]
    pub fn to_animated(&self, ambient_text: TextStyle) -> AnimatedLook {
        AnimatedLook {
            background: self.background.clone(),
            text: self.text.apply(&ambient_text),
        }
    }

    /// Visits this look's text overrides; destructured so a new field fails to compile.
    pub(crate) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            text,
            background: _,
        } = self;
        f(ThemeText::Overrides(text));
    }
}
