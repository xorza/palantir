//! One baked-icon draw.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::icon_ref::IconRef;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::spin::Spin;

/// One baked-icon draw, in logical px; physical box, raster size and atlas slot are decided downstream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawIconPayload {
    /// Fit-resolved paint rect.
    pub(crate) rect: Rect,
    pub(crate) icon: IconRef,
    /// Whole tint for a tintable icon, alpha only for a color one.
    pub(crate) tint: RgbaF16,
    /// Draw a color icon as its own luminance.
    pub(crate) desaturate: bool,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
}

impl PaintPayload for DrawIconPayload {
    /// This draw with alpha scaled by `by`, for the [`PaintSink`] gate.
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            tint: self.tint.faded(by),
            ..self
        }
    }

    /// Paints nothing for a zero-extent rect or fully transparent tint.
    #[inline]
    fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.tint.is_noop()
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.icon(self);
    }
}
