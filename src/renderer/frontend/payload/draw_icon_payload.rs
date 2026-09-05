//! One baked-icon draw.

use crate::icons::icon_set::IconRef;
use crate::primitives::color::RgbaF16;
use crate::primitives::rect::Rect;

/// One baked-icon draw, in logical px.
///
/// Deliberately small and deliberately unresolved: the physical box, the
/// raster size, and the atlas slot are all decided downstream, because none of
/// them are known until the display scale and ancestor transforms have been
/// applied. What the encoder settles is which icon, where, and in what colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawIconPayload {
    /// Fit-resolved paint rect in logical px.
    pub(crate) rect: Rect,
    pub(crate) icon: IconRef,
    /// Whole tint for a tintable icon, alpha only for a colour one.
    pub(crate) tint: RgbaF16,
    /// Draw a colour icon as its own luminance.
    pub(crate) desaturate: bool,
}

impl DrawIconPayload {
    /// This draw with its alpha scaled by `by`, for
    /// [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)'s
    /// gate.
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

    /// Paints nothing when the rect has no extent or the tint is fully
    /// transparent — the latter covering both icon kinds, since alpha gates
    /// the colour path as much as the mask one.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.tint.is_noop()
    }
}
