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
use glam::Vec2;

/// What a backdrop reads: what is painted under it, by the ticket the backdrop ledger wrote for it,
/// or the window's ground, while transparency is reduced, with no ticket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BackdropDraw {
    Capture(BackdropTicket),
    Ground,
}

/// The shape a material covers, in logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BackdropOutline {
    /// A rounded rect covering `coverage` of itself.
    Rect {
        coverage: Coverage,
        rect: Rect,
        corners: Corners,
    },
    /// A triangle placed as a triangle quad is: its corner centers, dilated by `radius`.
    Triangle { points: [Vec2; 3], radius: f32 },
}

/// A material's shape, showing what it reads through `material` under its fill, a color or a
/// gradient, and its border over the whole shape, faded to `opacity`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawBackdropPayload {
    pub(crate) outline: BackdropOutline,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    pub(crate) fill: GpuFill,
    /// The fill's gradient geometry in the box's 0..1 frame; zero for a solid.
    pub(crate) fill_axis: FillAxis,
    /// Normalized by [`LoweredBorder::normalized`], so `NONE` means no border exactly.
    pub(crate) border: LoweredBorder,
    pub(crate) material: LoweredMaterial,
    /// How much of the box shows, the fill and the material alike.
    pub(crate) opacity: f32,
    pub(crate) read: BackdropDraw,
}

impl DrawBackdropPayload {
    /// The shape as a quad packs it, which the backdrop's effect instance takes its rect, shape,
    /// turn and border width from, so it lies on the pixels a quad of it does.
    pub(crate) fn geometry(&self) -> DrawQuadPayload {
        let (geometry, kind) = match self.outline {
            BackdropOutline::Rect {
                coverage,
                rect,
                corners,
            } => (
                QuadGeometry::Rect {
                    rect,
                    corners,
                    spin: self.spin,
                },
                match coverage {
                    Coverage::Inside => FillKind::SOLID,
                    Coverage::Outside => FillKind::SOLID.with_outside(),
                },
            ),
            BackdropOutline::Triangle {
                points: [a, b, c],
                radius,
            } => (
                QuadGeometry::Triangle {
                    a,
                    b,
                    c,
                    radius,
                    spin: self.spin,
                },
                FillKind::SOLID.with_triangle(),
            ),
        };
        DrawQuadPayload {
            geometry,
            fill: GpuFill {
                color: self.fill.color,
                kind,
                lut_row: LutRow::FALLBACK,
            },
            border: self.border,
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
            border: self.border.faded(by),
            opacity: self.opacity * by,
            ..self
        }
    }

    /// Whether the backdrop draws nothing: it covers no pixel, or none of it shows, as a paint
    /// animation's alpha reads it.
    #[inline]
    fn is_noop(&self) -> bool {
        let empty = match self.outline {
            BackdropOutline::Rect { rect, .. } => rect.is_paint_empty(),
            // Its covering rect exists only after transform, and `TriangleShape::is_noop` filters
            // degenerates, as a triangle quad's.
            BackdropOutline::Triangle { .. } => false,
        };
        empty || is_invisible(self.opacity)
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.backdrop(self);
    }
}
