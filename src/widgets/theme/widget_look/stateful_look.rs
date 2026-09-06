//! The four-state look pack — normal, hovered, active, disabled — that
//! every state-styled widget theme is built from.

use crate::input::response::response_state::ResponseState;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;

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
    /// At rest.
    pub normal: WidgetLook,
    /// Under the pointer.
    pub hovered: WidgetLook,
    /// Engaged — pressed, or focused for a field. See [`Self::pick`].
    pub active: WidgetLook,
    /// Disabled, which outranks the other three.
    pub disabled: WidgetLook,
}

impl StatefulLook {
    /// Uniform pick precedence: disabled > active > hovered > normal.
    /// `active` is the widget's engaged flag (`state.pressed()` for
    /// press-driven widgets, `state.focused` for focus-driven ones);
    /// `disabled` / `hovered` read straight from `state`.
    #[inline(always)]
    pub fn pick(&self, state: &ResponseState, active: bool) -> &WidgetLook {
        if state.disabled {
            &self.disabled
        } else if active {
            &self.active
        } else if state.hovered() {
            &self.hovered
        } else {
            &self.normal
        }
    }

    /// Destructured so a new state fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self {
            normal,
            hovered,
            active,
            disabled,
        } = self;
        normal.for_each_text(f);
        hovered.for_each_text(f);
        active.for_each_text(f);
        disabled.for_each_text(f);
    }
}
