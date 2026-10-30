//! [`SlotDefaults`]: what a themed widget sets on its node beside its look.

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::spacing::Spacing;

/// What a themed widget contributes to the node rather than the paint: default
/// spacing and the transition spec. Themed bundles `#[serde(flatten)]` it.
/// Named fields because `padding` and `margin` share a type.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SlotDefaults {
    /// Padding the widget takes when its builder set none; explicit zero overrides it.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
    /// Margin when the builder set none.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::margin")]
    pub margin: Spacing,
    /// Spec the state transitions run under; `None` snaps, and a theme file that leaves it out
    /// reads `None`. The stock buttons, menu-bar titles, menu rows and tab chips take
    /// [`Metrics::transition`](crate::Metrics::transition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<AnimationSpec>,
}
