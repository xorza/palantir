//! Wheel, touchpad and pinch deltas as they reach one widget.

use crate::input::zoom_factor::ZoomFactor;
use glam::Vec2;

/// Wheel / touchpad / pinch deltas routed to the widget this frame; non-identity only when the widget has
/// [`Sense::SCROLL`](crate::input::sense::Sense::SCROLL) / [`Sense::PINCH`](crate::input::sense::Sense::PINCH) and
/// was the topmost routed target when the event arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollDelta {
    /// Pixel-precise delta in logical pixels (touchpads, precision wheels), negated at ingest so `+y` advances the
    /// scroll offset. [`Self::pan`] folds it with [`Self::lines`].
    pub pixels: Vec2,
    /// Notched delta in raw line units (not pixels) from classic wheels; sign matches [`Self::pixels`].
    pub lines: Vec2,
    /// Multiplicative pinch zoom ([`ZoomFactor::ONE`] = no pinch), reported without modifier gating, unlike wheel zoom.
    /// Fold it into a held zoom with [`ZoomFactor::combine`](crate::ZoomFactor::combine) to keep it in range.
    pub zoom: ZoomFactor,
}

impl ScrollDelta {
    /// This frame's pan in logical pixels: the precision source plus the notched one at `line_height`; each widget
    /// picks its own line height (`Scroll` the theme's, `TextEdit` its font's).
    #[inline]
    pub fn pan(self, line_height: f32) -> Vec2 {
        self.pixels + self.lines * line_height
    }
}

/// Hand-rolled because `zoom`'s identity is [`ZoomFactor::ONE`], not the derived zero.
impl Default for ScrollDelta {
    fn default() -> Self {
        Self {
            pixels: Vec2::ZERO,
            lines: Vec2::ZERO,
            zoom: ZoomFactor::ONE,
        }
    }
}
