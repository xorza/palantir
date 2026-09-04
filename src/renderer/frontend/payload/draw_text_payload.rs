//! One shaped-text run draw.

use crate::primitives::color::RgbaF16;
use crate::primitives::rect::Rect;
use crate::text::shaped_ref::ShapedTextRef;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawTextPayload {
    pub(crate) rect: Rect,
    pub(crate) color: RgbaF16,
    pub(crate) text: ShapedTextRef,
}

impl DrawTextPayload {
    /// Paints nothing when: zero-extent rect
    /// or fully transparent color. See [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)
    /// for the noop policy.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.color.is_noop()
    }
}
