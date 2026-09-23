//! The bundle a widget wears whole: its per-state looks, and the rule that
//! picks one of them from a response.

use crate::animation::anim_spec::AnimSpec;
use crate::input::response::response_state::ResponseState;
use crate::primitives::spacing::Spacing;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::look_plan::LookPlan;

/// A theme bundle a widget wears whole: the per-state looks its response
/// picks from, and the box defaults around them.
///
/// [`Self::plan`] is the route from a bundle to a [`LookPlan`], and
/// [`LookPlan::apply`] the route from the plan to a painted look — the two
/// calls every themed widget in the crate makes, and the two a widget of
/// your own makes to wear a theme bundle the same way:
///
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
/// A bundle that grows a fifth box default grows it in [`SlotDefaults`],
/// and the compiler names every implementor.
pub trait ThemeSlot {
    /// What the state pick needs past the response. `()` for the
    /// press-driven and focus-driven bundles; the toggles pass their
    /// checked flag, which selects between two four-state packs.
    type Pick: Copy;

    /// The look this bundle holds for `response`'s state, under `pick`.
    fn look(&self, response: &ResponseState, pick: Self::Pick) -> &WidgetLook;

    /// The spacing and transition spec the bundle contributes to the node.
    fn defaults(&self) -> SlotDefaults;

    /// Flatten into the owned plan [`LookPlan::apply`] consumes, resolving
    /// the ambient `text` fallback against the picked look.
    ///
    /// Read under the theme borrow. The result owns everything it carries,
    /// so the borrow ends here and the caller can reborrow the `Ui`
    /// mutably to animate toward it.
    // Same reason as `LookPlan::apply`, which this feeds: the chain crosses
    // the theme/widget codegen-unit boundary, and the default inliner leaves
    // the resolver and these accessors outlined in release builds.
    #[inline(always)]
    fn plan(&self, response: &ResponseState, pick: Self::Pick, text: TextStyle) -> LookPlan {
        LookPlan {
            target: self.look(response, pick).to_animated(text),
            defaults: self.defaults(),
        }
    }
}

/// What a themed widget contributes to the node rather than to the paint:
/// the spacing a widget takes when its builder set none, and the spec the
/// state transitions run under.
///
/// **Held by every themed bundle, not rebuilt from loose fields.** The
/// four bundles that have one — button, text edit, menu item, toggle —
/// carry this whole and `#[serde(flatten)]` it, so the triple is declared
/// once, documented once, and reaches the record pass without a copy per
/// field. Named fields rather than a constructor because `padding` and
/// `margin` are the same type and adjacent — a positional one is a swap
/// that compiles.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SlotDefaults {
    /// Padding the widget takes when its builder set none. Applied at
    /// `show()` time; explicit zero spacing overrides it.
    pub padding: Spacing,
    /// Margin the widget takes when its builder set none.
    pub margin: Spacing,
    /// Spec the state transitions run under. `None` by default —
    /// animation is opt-in. Round-trips through serde, so a theme file
    /// configures motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<AnimSpec>,
}
