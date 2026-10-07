//! The four-state look pack (normal, hovered, active, disabled) every state-styled widget theme is built from.

use crate::input::interaction::response_state::ResponseState;
use crate::widget_core::widget_look::WidgetLook;
use crate::widgets::theme::ThemeText;

/// The four-state look pack of state-styled widget themes. `active` is the widget's *engaged* state (pressed for Button / Toggle / MenuItem, focused for TextEdit), supplied as [`Self::pick`]'s flag so precedence is uniform. Embedded (serde-flattened) by [`crate::ButtonTheme`], [`crate::TextEditTheme`], [`crate::MenuItemTheme`]; [`crate::ToggleTheme`] keeps one per checked state.
// Not `Copy` because `WidgetLook` isn't.
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
    /// Pick precedence: disabled > active > hovered > normal. `active` is the widget's engaged flag; `disabled` / `hovered` come from `state`.
    #[inline(always)]
    pub const fn pick(&self, state: &ResponseState, active: bool) -> &WidgetLook {
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

    /// Destructured so a new state fails to compile; see [`Theme::for_each_text`](crate::Theme).
    pub(crate) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
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
