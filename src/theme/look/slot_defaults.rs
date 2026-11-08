//! [`SlotDefaults`]: what a themed widget sets on its node beside its look.

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::spacing::Spacing;

/// What a themed widget contributes to the node rather than the paint: default
/// spacing and the transition spec. Themed bundles `#[serde(flatten)]` it.
/// Named fields because `padding` and `margin` share a type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct SlotDefaults {
    /// Padding the widget takes when its builder set none; explicit zero overrides it.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub padding: Spacing,
    /// Margin when the builder set none.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::margin")]
    pub margin: Spacing,
    /// Spec the state transitions run under; a theme file that leaves it out reads
    /// [`AnimationSpec::SNAP`]. The stock buttons, menu-bar titles, menu rows and tab chips take
    /// [`Metrics::transition`](crate::Metrics::transition).
    #[serde(default, skip_serializing_if = "AnimationSpec::is_instant")]
    #[animate(snap)]
    pub transition: AnimationSpec,
}
