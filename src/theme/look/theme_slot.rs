//! The bundle a widget wears whole and the rule picking one look from a response.

use crate::input::interaction::response_state::ResponseState;
use crate::theme::look::WidgetLook;
use crate::theme::look::look_plan::LookPlan;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::text_style::TextStyle;

/// A theme bundle a widget wears whole: per-state looks plus box defaults.
/// [`Self::plan`] then [`Widget::wear`](crate::widget::Widget::wear) are the two calls every
/// themed widget makes:
/// ```
/// # use palantir::widget::{ThemeSlot, Widget};
/// # use palantir::Ui;
/// # fn demo(ui: &mut Ui) {
/// let mut widget = Widget::leaf();
/// let response = widget.response(ui);
/// let theme = ui.theme();
/// let plan = theme.button.plan(&response, (), theme.text);
/// let look = widget.wear(ui, plan);
/// widget.record(ui, Some(&look.chrome), |_| {});
/// # }
/// ```
///
/// A new box default goes in [`SlotDefaults`]; the compiler names every implementor.
pub trait ThemeSlot {
    /// What the state pick needs past the response: `()` normally, which look pack applies for toggles ([`CheckState`](crate::CheckState)) and tab chips ([`TabSelection`](crate::TabSelection)).
    type Pick: Copy;

    /// The look for the response state and pick.
    fn look(&self, response: &ResponseState, pick: Self::Pick) -> &WidgetLook;

    /// Slot defaults.
    fn defaults(&self) -> SlotDefaults;

    /// Flattens into the owned plan [`Widget::wear`](crate::widget::Widget::wear) consumes,
    /// folding the picked look's text overrides onto the ambient `text`; the result owns
    /// everything, so the theme borrow ends.
    // Cross-codegen-unit chain like `Widget::wear`; the default inliner leaves it outlined.
    #[inline(always)]
    fn plan(&self, response: &ResponseState, pick: Self::Pick, text: TextStyle) -> LookPlan {
        LookPlan {
            target: self.look(response, pick).to_animated(text),
            defaults: self.defaults(),
        }
    }
}
