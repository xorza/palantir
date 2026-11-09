//! A material's draw: a box over what it reads, and what the frame does with it.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, QuadGeometry};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::LoweredMaterial;

/// What a backdrop reads: what is painted under it, by the ticket the backdrop ledger wrote for it,
/// or the window's ground, while transparency is reduced, with no ticket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BackdropDraw {
    Capture(BackdropTicket),
    Ground,
}

/// A material's box covering `coverage` of `rect`, showing what it reads through `material` under
/// its fill, a color or a gradient, and its border over the whole box, faded to `opacity`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawBackdropPayload {
    pub(crate) coverage: Coverage,
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    pub(crate) fill: GpuFill,
    /// The fill's gradient geometry in the box's 0..1 frame; zero for a solid.
    pub(crate) fill_axis: FillAxis,
    /// Normalized by [`LoweredBorder::normalized`], so `NONE` means no stroke exactly.
    pub(crate) stroke: LoweredBorder,
    pub(crate) material: LoweredMaterial,
    /// How much of the box shows, the fill and the material alike.
    pub(crate) opacity: f32,
    pub(crate) read: BackdropDraw,
}

impl DrawBackdropPayload {
    /// The rect as a quad packs it, which the backdrop's effect instance takes its rect, corners,
    /// turn and stroke width from, so it lies on the pixels a rect of it does.
    pub(crate) fn geometry(&self) -> DrawQuadPayload {
        let kind = match self.coverage {
            Coverage::Inside => FillKind::SOLID,
            Coverage::Outside => FillKind::SOLID.with_window(),
        };
        DrawQuadPayload {
            geometry: QuadGeometry::Rect {
                rect: self.rect,
                corners: self.corners,
                spin: self.spin,
            },
            fill: GpuFill {
                color: self.fill.color,
                kind,
                lut_row: LutRow::FALLBACK,
            },
            stroke: self.stroke,
            fill_axis: FillAxis::from_lanes([0.0, 0.0, 0.0, self.opacity]),
        }
    }
}

impl PaintPayload for DrawBackdropPayload {
    /// This backdrop faded by `by`.
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            fill: self.fill.faded(by),
            stroke: self.stroke.faded(by),
            opacity: self.opacity * by,
            ..self
        }
    }

    /// Whether the backdrop draws nothing: it covers no pixel, or none of it shows, as a paint
    /// animation's alpha reads it.
    #[inline]
    fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || is_invisible(self.opacity)
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.backdrop(self);
    }
}
