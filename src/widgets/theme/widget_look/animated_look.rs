use crate::primitives::background::Background;
use crate::widgets::theme::text_style::TextStyle;
use palantir_anim_derive::Animatable;

/// Resolved + per-frame animated values for a [`WidgetLook`]. Built
/// by [`WidgetLook::animate`]. Widgets read `background` and `text`
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

impl AnimatedLook {
    /// Convenience: `text.line_height_for(text.font_size_px)`. Widgets
    /// rendering `ShapeRecord::Text` need this paired with `font_size_px`
    /// for the shaper.
    pub fn line_height_px(&self) -> f32 {
        self.text.line_height_for(self.text.font_size_px)
    }
}
