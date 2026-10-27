//! The four-state look pack (normal, hovered, active, disabled) every state-styled widget theme is built from.

use crate::input::interaction::response_state::ResponseState;
use crate::theme::ThemeText;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;

/// The four-state look pack of state-styled widget themes. `active` is the widget's *engaged* state (pressed for Button / Toggle / MenuItem, focused for TextEdit), named to [`Self::pick`] as an [`Engagement`] so precedence is uniform. Embedded (serde-flattened) by [`crate::ButtonTheme`], [`crate::TextEditTheme`], [`crate::MenuItemTheme`]; [`crate::ToggleTheme`] keeps one per checked state.
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
    /// Pick precedence: disabled > active > hovered > normal, all read from `state`; `engagement` names the state that counts as active.
    #[inline(always)]
    pub const fn pick(&self, state: &ResponseState, engagement: Engagement) -> &WidgetLook {
        let engaged = match engagement {
            Engagement::Pressed => state.pressed(),
            Engagement::Focused => state.focused,
        };
        if state.disabled {
            &self.disabled
        } else if engaged {
            &self.active
        } else if state.hovered() {
            &self.hovered
        } else {
            &self.normal
        }
    }

    /// Every state's look, for a restyle that touches them all; destructured, so a state added
    /// to the look reaches every caller.
    pub const fn each_mut(&mut self) -> [&mut WidgetLook; 4] {
        let Self {
            normal,
            hovered,
            active,
            disabled,
        } = self;
        [normal, hovered, active, disabled]
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
