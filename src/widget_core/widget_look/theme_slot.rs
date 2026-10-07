//! The bundle a widget wears whole and the rule picking one look from a response.

use crate::animation::animation_spec::AnimationSpec;
use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::spacing::Spacing;
use crate::widget_core::widget_look::WidgetLook;
use crate::widget_core::widget_look::look_plan::LookPlan;
use crate::widgets::theme::text_style::TextStyle;

/// A theme bundle a widget wears whole: per-state looks plus box defaults.
/// [`Self::plan`] then [`LookPlan::apply`] are the two calls every themed widget makes:
/// ```
/// # use palantir::widget::{ThemeSlot, Widget};
/// # use palantir::Ui;
/// # fn demo(ui: &mut Ui) {
/// let mut widget = Widget::leaf();
/// let response = widget.response(ui);
/// let theme = ui.theme();
/// let look = theme
///     .button
///     .plan(&response, (), theme.text)
///     .apply(ui, &mut widget);
/// widget.record(ui, Some(&look.background), |_| {});
/// # }
/// ```
///
/// A new box default goes in [`SlotDefaults`]; the compiler names every implementor.
pub trait ThemeSlot {
    /// What the state pick needs past the response: `()` normally, the checked flag for toggles.
    type Pick: Copy;

    /// The look for the response state and pick.
    fn look(&self, response: &ResponseState, pick: Self::Pick) -> &WidgetLook;

    /// Slot defaults.
    fn defaults(&self) -> SlotDefaults;

    /// Flattens into the owned plan [`LookPlan::apply`] consumes, folding the
    /// picked look's text overrides onto the ambient `text`; the result owns
    /// everything, so the theme borrow ends.
    // Cross-codegen-unit chain like `LookPlan::apply`; the default inliner leaves it outlined.
    #[inline(always)]
    fn plan(&self, response: &ResponseState, pick: Self::Pick, text: TextStyle) -> LookPlan {
        LookPlan {
            target: self.look(response, pick).to_animated(text),
            defaults: self.defaults(),
        }
    }
}

/// What a themed widget contributes to the node rather than the paint: default
/// spacing and the transition spec. Themed bundles `#[serde(flatten)]` it.
/// Named fields because `padding` and `margin` share a type.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SlotDefaults {
    /// Padding the widget takes when its builder set none; explicit zero overrides it.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::margin")]
    /// Margin when the builder set none.
    pub margin: Spacing,
    /// Spec the state transitions run under; `None` (animation is opt-in) by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<AnimationSpec>,
}
