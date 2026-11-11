//! [`ClosedFill`]: a box's or a triangle's fill and border.

use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::{
    ClosedPaintPayload, DrawClosedPayload,
};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::paint::lowered_border::LoweredBorder;

/// A fill and a border over a closed shape, all lowered to a single `QuadInstance`.
///
/// `fill.kind` names the brush; the side a box covers and a triangle are the shape's, which the
/// composer writes into the instance. `fill_axis` is the gradient geometry in the shape's 0..1
/// frame. `fill.color` is the solid color; for a gradient it multiplies the atlas row's color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClosedFill {
    pub(crate) fill: GpuFill,
    /// Normalized by [`LoweredBorder::normalized`], so `NONE` means no border exactly.
    pub(crate) border: LoweredBorder,
    /// The gradient's axis; zeroed for a solid. Outside [`GpuFill`] because only a closed fill
    /// has one.
    pub(crate) fill_axis: FillAxis,
}

impl ClosedFill {
    /// `fill` under `border`. The brush is lowered here because the GPU lanes it fills are this
    /// type's.
    pub(crate) const fn new(fill: BrushSource, border: LoweredBorder) -> Self {
        Self {
            fill: fill.gpu_fill(),
            border: border.normalized(),
            fill_axis: fill.fill_axis(),
        }
    }
}

impl ClosedPaintPayload for ClosedFill {
    /// This fill with its alpha and its border's scaled by `by`.
    #[inline]
    fn faded(self, by: f32) -> Self {
        Self {
            fill: self.fill.faded(by),
            border: self.border.faded(by),
            ..self
        }
    }

    /// Paints nothing over a box of no extent, or where neither fill nor border can put down a
    /// texel. A triangle's covering rect exists only after transform, and its record and its
    /// placement drop one of no area.
    ///
    /// A gradient fill is never a no-op here: [`BrushSource::gpu_fill`] sets its color lane to
    /// white (the atlas row supplies the color), and `Brush::is_noop` filters all-transparent
    /// stops before lowering.
    #[inline]
    fn is_noop(&self, geometry: PlacedClosed) -> bool {
        let empty = match geometry {
            PlacedClosed::Box(placed) => placed.rect.is_paint_empty(),
            PlacedClosed::Triangle(_) => false,
        };
        empty || (self.fill.is_noop() && self.border.is_noop())
    }

    fn paint(draw: DrawClosedPayload<Self>, sink: &mut impl PaintSink) {
        sink.fill(draw);
    }
}

impl DrawClosedPayload<ClosedFill> {
    /// Whether this is a solid fill over a box: the one draw whose covered rect can be whole,
    /// which the clear fold and the occlusion prune read. A triangle's corners and a gradient's
    /// stops cover no rect whole.
    pub(crate) fn is_solid_box(&self) -> bool {
        matches!(self.geometry, PlacedClosed::Box(_)) && self.paint.fill.kind == FillKind::SOLID
    }
}

#[cfg(test)]
mod tests;
