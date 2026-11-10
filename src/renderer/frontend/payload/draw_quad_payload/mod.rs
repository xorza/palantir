//! The quad-tier draw: rounded rects, outside rects, box-shadows and
//! rounded triangles, which all lower to one `Quad` instance.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::geometry::triangle_geometry::PlacedTriangle;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_shadow::{LoweredShadow, ShadowGeometry};

/// One quad-tier draw (rounded rect, outside rect, box-shadow or rounded
/// triangle), all lowered to a single `Quad` instance; they differ in
/// [`geometry`](Self::geometry) and the fill's kind.
///
/// `fill.kind` names the brush, or a shadow by its tag; the side a box covers and a triangle are
/// the geometry's, which the composer writes into the instance. `fill_lut_row` is the gradient
/// atlas row, or [`LutRow::FALLBACK`]. `fill_axis` is the gradient geometry, or for a shadow
/// `(0, 0, σ, spread)` (drop) / `(offset.x, offset.y, σ, spread)` (inset) in logical px, scaled
/// to physical by the composer.
///
/// `fill` is the solid color (the tint for a shadow); for a gradient it
/// multiplies the atlas row's color. `RgbaF16` saves 8 B per payload over
/// `RgbaF32`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawQuadPayload {
    /// The shape in the logical frame; a shadow's is its source box, which the composer grows
    /// a drop shadow's paint rect from after snapping.
    pub(crate) geometry: PlacedClosed,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    pub(crate) fill: GpuFill,
    /// Normalized by [`LoweredBorder::normalized`], so `NONE` means no border exactly.
    pub(crate) border: LoweredBorder,
    /// The reused four-lane geometry slot: gradient axis or shadow `(offset,
    /// σ, spread)`. Outside [`GpuFill`] because only this tier has one.
    pub(crate) fill_axis: FillAxis,
}

/// What [`DrawQuadPayload::rect`] lowers: a rect covering `coverage` with its corner
/// radii, fill brush and border.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RectQuad {
    pub(crate) coverage: Coverage,
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
    pub(crate) fill: BrushSource,
    pub(crate) border: LoweredBorder,
}

/// What [`DrawQuadPayload::triangle`] lowers: a triangle placed in the logical frame, with its
/// fill brush and a border.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TriangleQuad {
    pub(crate) triangle: PlacedTriangle,
    pub(crate) fill: BrushSource,
    pub(crate) border: LoweredBorder,
}

impl DrawQuadPayload {
    /// A rect covering `q.coverage` with `q.fill` and `q.border`. The brush is lowered here
    /// because the GPU lanes it fills are this type's.
    pub(crate) const fn rect(q: RectQuad) -> Self {
        let RectQuad {
            coverage,
            rect,
            corners,
            fill,
            border,
        } = q;
        Self {
            geometry: PlacedClosed::Box {
                placed: PlacedBox { rect, corners },
                coverage,
            },
            spin: None,
            // Border stays solid-only.
            fill: fill.gpu_fill(),
            border: border.normalized(),
            fill_axis: fill.fill_axis(),
        }
    }

    /// This draw turned by `spin`.
    pub(crate) const fn turned(mut self, spin: Option<Spin>) -> Self {
        self.spin = spin;
        self
    }

    /// `shadow` cast by the source `rect`, or `None` when it paints nothing:
    /// its tint the fill, `fill_kind` `SHADOW_DROP|SHADOW_INSET`, `fill_axis`
    /// `(offset.x, offset.y, σ, spread)`, scaled to physical px by the composer.
    pub(crate) fn shadow(rect: Rect, corners: Corners, shadow: LoweredShadow) -> Option<Self> {
        if shadow.is_noop() {
            return None;
        }
        let kind = if shadow.inset() {
            FillKind::SHADOW_INSET
        } else {
            FillKind::SHADOW_DROP
        };
        Some(Self {
            geometry: PlacedClosed::inside(PlacedBox { rect, corners }),
            spin: None,
            fill: GpuFill {
                color: shadow.color,
                kind,
                lut_row: LutRow::FALLBACK,
            },
            // A shadow has no border; its edge is the blur.
            border: LoweredBorder::NONE,
            // The axis travels as the packed word; unpacking would round-trip identical bytes through f16.
            fill_axis: FillAxis::from(shadow.geometry_f16),
        })
    }

    /// A rounded triangle, `q.triangle` with `q.fill` and `q.border`. Same tier and border
    /// normalization as [`Self::rect`].
    pub(crate) const fn triangle(q: TriangleQuad) -> Self {
        let TriangleQuad {
            triangle,
            fill,
            border,
        } = q;
        Self {
            geometry: PlacedClosed::Triangle(triangle),
            spin: None,
            fill: fill.gpu_fill(),
            border: border.normalized(),
            fill_axis: fill.fill_axis(),
        }
    }

    /// Whether this is a solid fill over the inside of a box: the one draw whose covered rect
    /// can be whole, which the clear fold, the fragment fast path and the occlusion prune read.
    /// A shadow's blur, an outside rect's hole and a triangle's corners cover no rect whole.
    pub(crate) fn is_solid_box(&self) -> bool {
        matches!(
            self.geometry,
            PlacedClosed::Box {
                coverage: Coverage::Inside,
                ..
            }
        ) && self.fill.kind == FillKind::SOLID
    }

    /// Whether the geometry covers no pixels. A drop shadow covers its source
    /// grown by the halo. A triangle is `false`: its covering rect exists only
    /// after transform, and `TriangleShape::is_noop` filters degenerates.
    #[inline]
    fn is_paint_empty(&self) -> bool {
        match self.geometry {
            PlacedClosed::Box { placed, .. } if self.fill.kind == FillKind::SHADOW_DROP => {
                let halo = ShadowGeometry::from_lanes(self.fill_axis.lanes()).halo();
                placed.rect.inflated(halo).is_paint_empty()
            }
            PlacedClosed::Box { placed, .. } => placed.rect.is_paint_empty(),
            PlacedClosed::Triangle(_) => false,
        }
    }
}

impl PaintPayload for DrawQuadPayload {
    /// This draw with its alpha scaled by `by`, for
    /// [`PaintSink`]'s gate.
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            fill: self.fill.faded(by),
            border: self.border.faded(by),
            ..self
        }
    }

    /// Paints nothing when the geometry covers no pixels, or neither fill nor
    /// border can put down a texel.
    ///
    /// Shadow parameters are not gated: a zero-σ drop shadow still paints a
    /// hard shifted rect, and the box's lowering drops a shadow that paints nothing.
    ///
    /// A gradient fill is never a no-op here: [`BrushSource::gpu_fill`] sets
    /// its color lane to white (the atlas row supplies the color), and
    /// `Brush::is_noop` filters all-transparent stops before lowering.
    #[inline]
    fn is_noop(&self) -> bool {
        self.is_paint_empty() || (self.fill.is_noop() && self.border.is_noop())
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.quad(self);
    }
}

#[cfg(test)]
mod tests;
