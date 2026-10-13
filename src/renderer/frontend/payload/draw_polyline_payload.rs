//! One stroked-polyline draw.

use crate::common::span::Span;
use crate::primitives::math::domain::is_invisible;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::shape::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;

/// Stroked polyline payload; `width` is logical px. Points and colors live in the window's
/// [`RecordStore`]; `colors` holds 1 (broadcast), `points.len` (per-point) or `points.len - 1`
/// (per-segment), per `color_mode`. Points are owner-local; the composer applies `origin` and inflates
/// `bounds` for stroke, cap, join and AA once in physical space.
///
/// [`RecordStore`]: crate::scene::record_store::RecordStore
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawPolylinePayload {
    pub(crate) bounds: StrokeBounds,
    pub(crate) origin: Vec2,
    pub(crate) width: f32,
    pub(crate) points: Span,
    pub(crate) colors: Span,
    pub(crate) color_mode: ColorMode,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    /// Paint-animation opacity, `1.0` when still; applied by the composer as it writes each instance.
    pub(crate) alpha: f32,
}

impl DrawPolylinePayload {
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            alpha: self.alpha * by,
            ..self
        }
    }

    /// Paints nothing: degenerate, or faded out. Colours are not checked (an O(n) span read); the
    /// bbox is not either, since a zero-area one can still paint stroke pixels.
    #[inline]
    pub(crate) const fn is_noop(&self) -> bool {
        self.is_degenerate() || is_invisible(self.alpha)
    }

    /// Fewer than two points, or an invisible stroke width: an invariant (`PaintSink::draw_polyline`
    /// asserts it), not a filter.
    #[inline]
    pub(crate) const fn is_degenerate(&self) -> bool {
        self.points.len < 2 || is_invisible(self.width)
    }
}
