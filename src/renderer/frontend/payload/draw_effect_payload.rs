//! One effect draw.

use crate::common::span::Span;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};

/// An effect over a rounded rect in the logical frame; `params` indexes the window's
/// [`RecordStore`](crate::shape::record_store::RecordStore) parameter words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawEffectPayload {
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
    pub(crate) effect: EffectId,
    pub(crate) params: Span,
    /// Its images, indexed into the record store's image column.
    pub(crate) images: Span,
    /// A paint animation's fade, which scales the coverage.
    pub(crate) opacity: f32,
    /// Seconds into the paint animation's pass, which the effect reads; 0 when unanimated.
    pub(crate) time: f32,
}

impl PaintPayload for DrawEffectPayload {
    #[inline]
    fn faded(self, by: f32) -> Self {
        Self {
            opacity: self.opacity * by,
            ..self
        }
    }

    /// Paints nothing over an empty rect or at no opacity.
    #[inline]
    fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.opacity <= 0.0
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.effect(self);
    }
}
