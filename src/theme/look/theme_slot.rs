//! The bundle a widget wears whole and the rule picking one look from a response.

use crate::animation::animation_key::AnimationKey;
use crate::primitives::input::interaction::response_state::ResponseState;
use crate::theme::look::WidgetLook;
use crate::theme::look::look_plan::LookPlan;
use crate::theme::look::press_edge::PressEdge;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::text_style::TextStyle;
use std::hash::Hash;

/// A theme bundle a widget wears whole: per-state looks plus box defaults.
/// [`Self::plan`] then [`Widget::wear`](crate::widget::Widget::wear) are the two calls every
/// themed widget makes:
/// ```
/// # use palantir::widget::{ThemeSlot, Widget};
/// # use palantir::{Selection, Ui};
/// # fn demo(ui: &mut Ui) {
/// let mut widget = Widget::leaf();
/// let response = widget.response(ui);
/// let theme = ui.theme();
/// let plan = theme.button.plan(&response, Selection::Unselected, theme.text);
/// let look = widget.wear(ui, plan);
/// widget.record(ui, Some(&look.chrome), |_| {});
/// # }
/// ```
///
/// A new box default goes in [`SlotDefaults`]; the compiler names every implementor.
pub trait ThemeSlot {
    /// What the state pick needs past the response: `()` normally, which look pack applies for toggles ([`CheckState`](crate::CheckState)) and tab chips and buttons ([`Selection`](crate::Selection)).
    type Pick: Copy + Hash;

    /// The look for the response state and pick. It reads only the pick and the response's
    /// `disabled`, `pressed()`, `focused` and `hovered()`, which key the look's animation in
    /// [`Self::plan`]: a look that turned on anything else would change under one key and be
    /// followed at once instead of fading.
    fn look(&self, response: &ResponseState, pick: Self::Pick) -> &WidgetLook;

    /// Slot defaults.
    fn defaults(&self) -> SlotDefaults;

    /// Flattens into the owned plan [`Widget::wear`](crate::widget::Widget::wear) consumes,
    /// folding the picked look's text overrides onto the ambient `text`; the result owns
    /// everything, so the theme borrow ends. The plan's key is the pick and every response bit a
    /// look can turn on, so a look that changes with the theme alone is followed, not animated
    /// toward; a key finer than the looks only retargets toward an equal look, which is no
    /// motion.
    // Cross-codegen-unit chain like `Widget::wear`; the default inliner leaves it outlined.
    #[inline(always)]
    fn plan(&self, response: &ResponseState, pick: Self::Pick, text: TextStyle) -> LookPlan {
        LookPlan {
            target: self.look(response, pick).to_animated(text),
            key: AnimationKey::of((
                pick,
                response.disabled,
                response.pressed(),
                response.focused,
                response.hovered(),
            )),
            defaults: self.defaults(),
            press: PressEdge::of(response),
        }
    }
}
