//! A backdrop pane's draw: a rect over the pixels painted before it, and what the frame does
//! with it.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, QuadGeometry};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::pane_captures::PaneTicket;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::rect::RectKind;

/// A pane of `kind` at `rect`, showing the pixels painted before it through `backdrop` under a
/// solid `tint` and a stroke, faded to `opacity`, and the ticket the pane ledger wrote for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawPanePayload {
    pub(crate) kind: RectKind,
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    pub(crate) tint: RgbaF16,
    /// Normalized by [`ShapeStroke::normalized`], so `NONE` means no stroke exactly.
    pub(crate) stroke: ShapeStroke,
    /// Not [`LoweredBackdrop::NONE`]: a rect with no backdrop is an ordinary quad.
    pub(crate) backdrop: LoweredBackdrop,
    /// How much of the pane shows, the tint and the backdrop alike.
    pub(crate) opacity: f32,
    pub(crate) ticket: PaneTicket,
}

impl DrawPanePayload {
    /// The pane's quad: `FillKind::BACKDROP`, with the backdrop's look and the opacity in the
    /// axis lanes.
    pub(crate) fn quad(&self) -> DrawQuadPayload {
        debug_assert!(
            !self.backdrop.is_none(),
            "a pane with no backdrop is a rect"
        );
        let mut kind = FillKind::BACKDROP;
        if self.kind == RectKind::Windowed {
            kind = kind.with_window();
        }
        DrawQuadPayload {
            geometry: QuadGeometry::Rect {
                rect: self.rect,
                corners: self.corners,
                spin: self.spin,
            },
            fill: GpuFill {
                color: self.tint,
                kind,
                lut_row: LutRow::FALLBACK,
            },
            stroke: self.stroke,
            fill_axis: self.backdrop.fill_axis(self.opacity),
        }
    }
}

impl PaintPayload for DrawPanePayload {
    /// This pane faded by `by`.
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            tint: self.tint.faded(by),
            stroke: self.stroke.faded(by),
            opacity: self.opacity * by,
            ..self
        }
    }

    /// Whether the pane draws nothing: it covers no pixel, or none of it shows, as a paint
    /// animation's alpha reads it.
    #[inline]
    fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || is_invisible(self.opacity)
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.pane(self);
    }
}
