//! One baked-icon draw.

use crate::icons::icon_set::IconRef;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::rgba_f16::RgbaF16;

/// One baked-icon draw, in logical px; physical box, raster size and atlas slot are decided downstream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawIconPayload {
    /// Fit-resolved paint rect.
    pub(crate) rect: Rect,
    pub(crate) icon: IconRef,
    /// Whole tint for a tintable icon, alpha only for a colour one.
    pub(crate) tint: RgbaF16,
    /// Draw a colour icon as its own luminance.
    pub(crate) desaturate: bool,
}

impl DrawIconPayload {
    /// This draw with alpha scaled by `by`, for the [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink) gate.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
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
    pub(crate) const fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.tint.is_noop()
    }
}
