//! A material's draw: a box over what it reads, and what the frame does with it.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, QuadGeometry};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::pane_captures::PaneTicket;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::rect::RectKind;

/// What a backdrop reads: what is painted under it, by the ticket the pane ledger wrote for it, or
/// the window's ground, while transparency is reduced, with no ticket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BackdropDraw {
    Capture(PaneTicket),
    Ground,
}

/// A material's box of `kind` at `rect`, showing what it reads through `material` under a solid
/// `tint` and a stroke, faded to `opacity`.
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
    pub(crate) material: LoweredMaterial,
    /// How much of the box shows, the tint and the material alike.
    pub(crate) opacity: f32,
    pub(crate) read: BackdropDraw,
}

impl DrawPanePayload {
    /// The rect as a quad packs it, which the pane's effect instance takes its rect, corners,
    /// turn and stroke width from, so it lies on the pixels a rect of it does.
    pub(crate) fn geometry(&self) -> DrawQuadPayload {
        let kind = match self.kind {
            RectKind::Rounded => FillKind::SOLID,
            RectKind::Windowed => FillKind::SOLID.with_window(),
        };
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
            fill_axis: FillAxis::from_lanes([0.0, 0.0, 0.0, self.opacity]),
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
