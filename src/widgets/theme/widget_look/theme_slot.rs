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
/// [`Self::plan`] is the only route from a bundle to a [`LookPlan`], and
/// the plan is the only route to a painted look. A bundle that grows a
/// fifth box default therefore grows it in [`SlotDefaults`], and the
/// compiler names every implementor — instead of the eight `show` bodies
/// that each spelled the quartet out.
pub(crate) trait ThemeSlot {
    /// What the state pick needs past the response. `()` for the
    /// press-driven and focus-driven bundles; the toggles pass their
    /// checked flag, which selects between two four-state packs.
    type Pick: Copy;

    fn look(&self, response: &ResponseState, pick: Self::Pick) -> &WidgetLook;

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
    fn plan(&self, response: &ResponseState, pick: Self::Pick, text: &TextStyle) -> LookPlan {
        LookPlan {
            target: self.look(response, pick).to_animated(text),
            defaults: self.defaults(),
        }
    }
}

/// What a [`ThemeSlot`] contributes to the node rather than to the paint:
/// the spacing a widget takes when its builder set none, and the spec the
/// state transitions run under.
///
/// Declared once so [`LookPlan`] carries it whole. Named fields rather
/// than a constructor because `padding` and `margin` are the same type and
/// adjacent — a positional one is a swap that compiles.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SlotDefaults {
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,
    pub(crate) anim: Option<AnimSpec>,
}
