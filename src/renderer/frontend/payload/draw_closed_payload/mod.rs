//! A paint over a closed shape, as one draw: [`DrawClosedPayload`], generic over the paint its
//! family draws, a fill, an image or an effect.

pub(crate) mod closed_effect;
pub(crate) mod closed_fill;
pub(crate) mod closed_image;

use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::closed_geometry::PlacedClosed;

/// `paint` over `geometry` in the logical frame, turned by a paint animation's `spin`. One type
/// for every closed paint, so the shape and the turn sit where they sit for each; generic over the
/// paint rather than an enum of them, as a fill draw would carry an effect's bytes. Refused: a
/// payload per paint that holds its own shape, as each did, an image's shape inside its paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawClosedPayload<P> {
    pub(crate) geometry: PlacedClosed,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    pub(crate) paint: P,
}

impl<P> DrawClosedPayload<P> {
    /// `paint` over `geometry`, unturned.
    pub(crate) const fn new(geometry: PlacedClosed, paint: P) -> Self {
        Self {
            geometry,
            spin: None,
            paint,
        }
    }

    /// This draw turned by `spin`.
    pub(crate) const fn turned(mut self, spin: Option<Spin>) -> Self {
        self.spin = spin;
        self
    }
}

/// What covers a closed shape in a [`DrawClosedPayload`]: it fades, says whether it paints over
/// its shape, and names the [`PaintSink`] method that takes its draw.
pub(crate) trait ClosedPaintPayload: Sized {
    /// This paint with its alpha scaled by `by`.
    fn faded(self, by: f32) -> Self;

    /// Whether it paints nothing over `geometry`.
    fn is_noop(&self, geometry: PlacedClosed) -> bool;

    /// Hand `draw` to the method of `sink` that takes its paint.
    fn paint(draw: DrawClosedPayload<Self>, sink: &mut impl PaintSink);
}

impl<P: ClosedPaintPayload> PaintPayload for DrawClosedPayload<P> {
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            paint: self.paint.faded(by),
            ..self
        }
    }

    #[inline]
    fn is_noop(&self) -> bool {
        self.paint.is_noop(self.geometry)
    }

    fn paint(self, sink: &mut impl PaintSink) {
        P::paint(self, sink);
    }
}
