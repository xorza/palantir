//! One frame's resolved look, after tweening between the state a widget leaves and enters.

use crate::primitives::paint::background::Background;
use crate::widgets::theme::text_style::TextStyle;
use palantir_anim_derive::Animatable;

/// Resolved per-frame animated values for a [`WidgetLook`](crate::WidgetLook), built by [`WidgetLook::to_animated`](crate::WidgetLook::to_animated). `text.color` is animated; `text.font_size` and `text.line_height_factor` snap-carry the look's overrides.
// Not `Copy`: `Background` isn't.
#[derive(Clone, Debug, Default, PartialEq, Animatable)]
pub struct AnimatedLook {
    /// The animated background.
    pub background: Background,
    /// The animated text style with the look's overrides folded in.
    pub text: TextStyle,
}
