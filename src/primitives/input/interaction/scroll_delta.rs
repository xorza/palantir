//! Wheel, touchpad and pinch deltas as they reach one widget.

use crate::primitives::geometry::size::Size;
use crate::primitives::input::interaction::wheel_delta::WheelDelta;
use crate::primitives::input::zoom_factor::ZoomFactor;
use glam::Vec2;

/// One page's travel along a viewport `view` long: all of it but a little, so a line of context
/// stays, and at least a pixel.
pub(crate) const fn page_step(view: f32) -> f32 {
    (view * 0.875).max(view - 40.0).max(1.0)
}

/// Wheel / touchpad / pinch deltas routed to the widget this frame; non-identity only when the widget has
/// [`Sense::SCROLL`](crate::primitives::input::sense::Sense::SCROLL) / [`Sense::PINCH`](crate::primitives::input::sense::Sense::PINCH) and
/// was the topmost routed target when the event arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollDelta {
    /// The frame's wheel and touchpad travel; [`Self::pan`] folds its pixels, lines and pages, and a
    /// wheel zoom reads its notches ([`ZoomFactor::from_wheel`](crate::ZoomFactor::from_wheel)).
    pub wheel: WheelDelta,
    /// Multiplicative pinch zoom ([`ZoomFactor::ONE`] = no pinch), reported without modifier gating, unlike wheel zoom.
    /// Fold it into a held zoom with [`ZoomFactor::combine`](crate::ZoomFactor::combine) to keep it in range.
    pub zoom: ZoomFactor,
}

impl ScrollDelta {
    /// Nothing routed: no pan and a pinch at [`ZoomFactor::ONE`].
    pub const NONE: Self = Self {
        wheel: WheelDelta::ZERO,
        zoom: ZoomFactor::ONE,
    };

    /// This frame's pan in logical pixels: the precision source plus the notched one at `line_height` and a page of
    /// `viewport`; each widget picks its own line height (`Scroll` the theme's, `TextEdit` its font's).
    #[inline]
    pub const fn pan(self, line_height: f32, viewport: Size) -> Vec2 {
        let WheelDelta {
            pixels,
            lines,
            pages,
            notches: _,
        } = self.wheel;
        Vec2::new(
            pixels.x + lines.x * line_height + pages.x * page_step(viewport.w),
            pixels.y + lines.y * line_height + pages.y * page_step(viewport.h),
        )
    }
}

/// Hand-rolled because `zoom`'s identity is [`ZoomFactor::ONE`], not the derived zero.
impl Default for ScrollDelta {
    fn default() -> Self {
        Self::NONE
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::size::Size;
    use crate::primitives::input::interaction::scroll_delta::{ScrollDelta, page_step};
    use crate::primitives::input::interaction::wheel_delta::WheelDelta;
    use glam::Vec2;

    /// A page keeps 40 px or an eighth of the view, whichever keeps less, and never reaches 0.
    #[test]
    fn a_page_is_most_of_the_view() {
        // 0.875 × 800 = 700 against 800 − 40 = 760.
        assert_eq!(page_step(800.0), 760.0);
        // 0.875 × 100 = 87.5 against 100 − 40 = 60.
        assert_eq!(page_step(100.0), 87.5);
        assert_eq!(page_step(0.0), 1.0);
    }

    /// Each lane adds its own step, and `notches` moves nothing.
    #[test]
    fn a_pan_folds_pixels_lines_and_pages() {
        let delta = ScrollDelta {
            wheel: WheelDelta {
                pixels: Vec2::new(1.0, 2.0),
                lines: Vec2::new(3.0, 4.0),
                notches: Vec2::new(100.0, 100.0),
                pages: Vec2::new(0.0, 2.0),
            },
            ..ScrollDelta::NONE
        };
        // x: 1 + 3 × 20 + 0 = 61. y: 2 + 4 × 20 + 2 × 760 = 1602.
        assert_eq!(
            delta.pan(20.0, Size::new(100.0, 800.0)),
            Vec2::new(61.0, 1602.0)
        );
    }
}
