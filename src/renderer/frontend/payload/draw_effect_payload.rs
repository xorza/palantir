//! One effect draw.

use crate::common::span::Span;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::draw_backdrop_payload::BackdropDraw;
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::shape_outline::PlacedOutline;

/// An effect over a rounded rect or a triangle in the logical frame; `params` indexes the window's
/// [`RecordStore`](crate::shape::record_store::RecordStore) parameter words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawEffectPayload {
    pub(crate) outline: PlacedOutline,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    pub(crate) effect: EffectId,
    pub(crate) params: Span,
    /// Its images, indexed into the record store's image column.
    pub(crate) images: Span,
    /// A backdrop effect's backdrop; `None` for a color effect.
    pub(crate) backdrop: Option<EffectCaptureDraw>,
    /// A paint animation's fade, which scales the coverage.
    pub(crate) opacity: f32,
    /// Seconds into the paint animation's pass, which the effect reads; 0 when unanimated.
    pub(crate) time: f32,
}

/// What a backdrop effect's draw reads, and how far past its rect it reads, in logical px; a
/// draw over the ground reads under its rect alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectCaptureDraw {
    pub(crate) read: BackdropDraw,
    pub(crate) reach: f32,
}

impl PaintPayload for DrawEffectPayload {
    #[inline]
    fn faded(self, by: f32) -> Self {
        Self {
            opacity: self.opacity * by,
            ..self
        }
    }

    /// Paints nothing over an empty rect or at no opacity. A triangle is never empty here: its
    /// covering rect exists only after transform, and its record filters degenerates.
    #[inline]
    fn is_noop(&self) -> bool {
        let empty = match self.outline {
            PlacedOutline::Rect { rect, .. } => rect.is_paint_empty(),
            PlacedOutline::Triangle { .. } => false,
        };
        empty || self.opacity <= 0.0
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.effect(self);
    }
}
