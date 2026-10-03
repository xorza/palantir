//! Wheel, touchpad and pinch deltas as they reach one widget, in the three
//! units the platforms send them in.

use crate::input::zoom_factor::ZoomFactor;
use glam::Vec2;

/// Wheel / touchpad / pinch deltas routed to the widget this frame.
/// Only non-identity when the widget has
/// [`Sense::SCROLL`](crate::input::sense::Sense::SCROLL) /
/// [`Sense::PINCH`](crate::input::sense::Sense::PINCH) AND was the
/// topmost routed target when an event arrived. Later pointer movement
/// does not reassign an accumulated delta.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollDelta {
    /// Pixel-precise scroll delta in logical pixels — the touchpad /
    /// precision-wheel source (winit `MouseScrollDelta::PixelDelta`).
    /// Already negated at ingest so `+y` means "advance the scroll
    /// offset forward." [`Self::pan`] folds it with [`Self::lines`].
    pub pixels: Vec2,
    /// Notched / line-discrete scroll delta in raw line units (NOT
    /// pixels) — the classic-wheel source (winit
    /// `MouseScrollDelta::LineDelta`). Sign matches [`Self::pixels`].
    /// Use for "mouse wheel" intent (e.g. zoom-by-notches in a graph
    /// viewport that pans on touchpad).
    pub lines: Vec2,
    /// Multiplicative pinch zoom factor ([`ZoomFactor::ONE`] = no
    /// pinch). Pinch always reports — no modifier gating, unlike wheel
    /// zoom, which the caller derives from [`Self::lines`] and the
    /// modifiers through [`ZoomFactor::from_wheel`].
    ///
    /// Fold it into a held view zoom with
    /// [`ZoomFactor::combine`](crate::ZoomFactor::combine), which is what
    /// keeps a long gesture from accumulating out of range.
    pub zoom: ZoomFactor,
}

impl ScrollDelta {
    /// This frame's pan in logical pixels: the precision source plus the
    /// notched one converted at `line_px`.
    ///
    /// **The one fold.** Each widget still chooses the line height it
    /// converts at — a `Scroll` takes the theme's, a `TextEdit` its own
    /// font's, so one notch advances each by its own lines — but what
    /// they do with it is this, spelled once rather than at every wheel
    /// reader.
    #[inline]
    pub fn pan(self, line_px: f32) -> Vec2 {
        self.pixels + self.lines * line_px
    }
}

/// Hand-rolled because `Vec2`'s identity is the zero `#[derive(Default)]`
/// gives and `zoom`'s is [`ZoomFactor::ONE`], so only two of the three
/// fields agree with the derive.
impl Default for ScrollDelta {
    fn default() -> Self {
        Self {
            pixels: Vec2::ZERO,
            lines: Vec2::ZERO,
            zoom: ZoomFactor::ONE,
        }
    }
}
