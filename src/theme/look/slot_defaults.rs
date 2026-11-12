//! [`SlotDefaults`]: what a themed widget sets on its node beside its look.

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::spacing::Spacing;
use crate::theme::look::press_effect::PressEffect;

/// What a themed widget contributes to the node rather than its look: default spacing, the
/// transition spec and the press effect, a theme file's `defaults` of each themed widget.
/// Named fields because `padding` and `margin` share a type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
#[serde(deny_unknown_fields)]
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
    /// The effect each press draws ([`PressEffect`]); none in a stock theme, and in a theme file
    /// that leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[animate(snap)]
    pub press: Option<PressEffect>,
}
