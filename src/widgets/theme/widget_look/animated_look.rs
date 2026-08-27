//! One frame's resolved look, after the tween between the state a widget
//! is leaving and the one it is entering.

use crate::primitives::background::Background;
use crate::widgets::theme::text_style::TextStyle;
use palantir_anim_derive::Animatable;

/// Resolved + per-frame animated values for a [`WidgetLook`](crate::WidgetLook). Built
/// by [`WidgetLook::to_animated`](crate::WidgetLook::to_animated). Widgets read `background` and `text`
/// directly; both fields are already-animated.
///
/// `text.color` is the animated color; `text.font_size_px` and
/// `text.line_height_mult` are snap-carried from the picked
/// `WidgetLook` (or the fallback) — see `TextStyle`'s
/// `#[animate(snap)]` markings.
// **Not `Copy`** because `Background` isn't.
#[derive(Clone, Debug, Default, PartialEq, Animatable)]
pub struct AnimatedLook {
    pub background: Background,
    pub text: TextStyle,
}
