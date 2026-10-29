//! One shaped-text run draw.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::spin::Spin;
use crate::text::shaped_ref::ShapedTextRef;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawTextPayload {
    /// The measured block, which places the glyphs.
    pub(crate) rect: Rect,
    /// How far the glyphs' ink reaches past `rect`, in its units; the run's scissor covers it.
    pub(crate) ink: Spacing,
    pub(crate) color: RgbaF16,
    pub(crate) text: ShapedTextRef,
    /// A paint animation's turn, its pivot in the same logical frame as `rect`.
    pub(crate) spin: Option<Spin>,
}

impl PaintPayload for DrawTextPayload {
    /// This draw with alpha scaled by `by`, for the [`PaintSink`] gate.
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            color: self.color.faded(by),
            ..self
        }
    }

    /// Paints nothing for a zero-extent rect or fully transparent color.
    #[inline]
    fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.color.is_noop()
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.text(self);
    }
}
