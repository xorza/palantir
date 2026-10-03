//! One shaped-text run draw.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::text::shaped_ref::ShapedTextRef;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawTextPayload {
    /// The measured block, which places the glyphs.
    pub(crate) rect: Rect,
    /// How far the glyphs' ink reaches past `rect`, in its units — what
    /// the run's scissor covers beyond it.
    pub(crate) ink: Spacing,
    pub(crate) color: RgbaF16,
    pub(crate) text: ShapedTextRef,
}

impl DrawTextPayload {
    /// This draw with its alpha scaled by `by`, for
    /// [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)'s
    /// gate.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            color: self.color.faded(by),
            ..self
        }
    }

    /// Paints nothing when: zero-extent rect
    /// or fully transparent color. See [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)
    /// for the noop policy.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.color.is_noop()
    }
}
