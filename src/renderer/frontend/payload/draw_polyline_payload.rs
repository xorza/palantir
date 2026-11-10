//! One stroked-polyline draw.

use crate::common::span::Span;
use crate::primitives::geometry::fit::FitMap;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::record::ColorMode;
use glam::Vec2;

/// Stroked polyline payload; `width` is logical px. Points and colors live in the window's
/// [`RecordStore`]; `colors` holds 1 (broadcast), `points.len` (per-point) or `points.len - 1`
/// (per-segment), per `color_mode`. Points are as recorded; the composer fits them by `fit` into the
/// owner-local frame, turns them by `spin`, then applies `origin`; it culls by `bbox`, already
/// fitted, turned and inflated for stroke, cap, join and AA once in physical space.
///
/// [`RecordStore`]: crate::shape::record_store::RecordStore
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawPolylinePayload {
    /// The fitted points' owner-local bounds, kept as a scan of the points is O(n).
    pub(crate) bbox: Rect,
    pub(crate) origin: Vec2,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    pub(crate) fit: FitMap,
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
    /// Fewer than two points, or an invisible stroke width: an invariant (`PaintPayload::check`
    /// asserts it), not a filter.
    #[inline]
    pub(crate) const fn is_degenerate(&self) -> bool {
        self.points.len < 2 || is_invisible(self.width)
    }
}

impl PaintPayload for DrawPolylinePayload {
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            alpha: self.alpha * by,
            ..self
        }
    }

    /// Paints nothing: degenerate, or faded out. Colors are not checked (an O(n) span read); the
    /// bbox is not either, since a zero-area one can still paint stroke pixels.
    #[inline]
    fn is_noop(&self) -> bool {
        self.is_degenerate() || is_invisible(self.alpha)
    }

    fn spin_mut(&mut self) -> &mut Option<Spin> {
        &mut self.spin
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.polyline(self);
    }

    /// Geometry is asserted, not gated: the polyline's lowering drops `< 2` points and a
    /// non-painting width, and the encoder forwards both verbatim. Other payloads
    /// gate because theirs are layout outputs that can legitimately collapse. Debug-only is safe:
    /// `is_noop` also covers the geometry, and the composer emits nothing for one.
    fn check(&self) {
        debug_assert!(
            !self.is_degenerate(),
            "degenerate polyline reached the sink — its lowering \
             should have dropped it: {self:?}",
        );
    }
}
