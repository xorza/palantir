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
    /// offset forward." Pair with [`Self::lines`] to form a combined
    /// pan delta: `pixels + lines * line_px`.
    pub pixels: Vec2,
    /// Notched / line-discrete scroll delta in raw line units (NOT
    /// pixels) — the classic-wheel source (winit
    /// `MouseScrollDelta::LineDelta`). Sign matches [`Self::pixels`].
    /// Use for "mouse wheel" intent (e.g. zoom-by-notches in a graph
    /// viewport that pans on touchpad).
    pub lines: Vec2,
    /// Multiplicative pinch zoom factor (`1.0` = no pinch). Pinch
    /// always reports — no modifier gating, unlike wheel zoom which
    /// the caller derives manually from [`Self::lines`] + modifiers.
    pub zoom: f32,
}

/// Hand-rolled because `zoom`'s identity is `1.0`, not the `0.0` that
/// `#[derive(Default)]` would produce — `(zoom - 1.0).abs() > eps` is
/// a safe presence check on a `Default`-constructed instance.
impl Default for ScrollDelta {
    fn default() -> Self {
        Self {
            pixels: Vec2::ZERO,
            lines: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}
