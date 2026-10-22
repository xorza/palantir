//! The quad-tier draw: rounded rects, windowed rects, box-shadows and
//! rounded triangles, which all lower to one `Quad` instance.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::shape::paint::lowered_shadow::{LoweredShadow, ShadowGeometry};
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::rect::RectKind;
use glam::Vec2;

/// The geometry half of a [`DrawQuadPayload`]. Rects, windowed rects and
/// shadows arrive as an already-resolved rect; a triangle's covering rect
/// only exists after its points are transformed, so it carries the points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum QuadGeometry {
    /// A logical-px rect and corner radii; a shadow's are the source shape's
    /// (the composer grows a drop shadow's paint rect from it after snapping).
    Rect { rect: Rect, corners: Corners },
    /// Owner-local corner points and rounding. The composer folds `origin`
    /// and the push-transform, scales to physical px, derives the covering
    /// AABB (points inflated by `radius + AA fringe`) and packs the points
    /// into the `corners` / `fill_axis` lanes.
    Triangle {
        origin: Vec2,
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
    },
}

/// One quad-tier draw (rounded rect, windowed rect, box-shadow or rounded
/// triangle), all lowered to a single `Quad` instance; they differ in
/// [`geometry`](Self::geometry) and the SDF `fill_kind` selects.
///
/// `fill_kind`'s low byte is the kind tag; bits 8..16 carry `Spread` for
/// gradients. `fill_lut_row` is the gradient atlas row, or
/// [`LutRow::FALLBACK`]. `fill_axis` is the gradient geometry, or for a
/// shadow `(0, 0, σ, spread)` (drop) / `(offset.x, offset.y, σ, spread)`
/// (inset) in logical px, scaled to physical by the composer. A triangle's
/// is overwritten by the composer.
///
/// `fill` is the solid color (the tint for a shadow); for a gradient it
/// multiplies the atlas row's color. `RgbaF16` saves 8 B per payload over
/// `RgbaF32`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawQuadPayload {
    pub(crate) geometry: QuadGeometry,
    pub(crate) fill: GpuFill,
    /// Normalized by [`ShapeStroke::normalized`], so `NONE` means no stroke exactly.
    pub(crate) stroke: ShapeStroke,
    /// The reused four-lane geometry slot: gradient axis, shadow `(offset, σ,
    /// spread)`, or unread for a triangle. Outside [`GpuFill`] because only
    /// this tier has one.
    pub(crate) fill_axis: FillAxis,
}

/// What [`DrawQuadPayload::rect`] lowers: a rect of `kind` with its corner
/// radii, fill brush and stroke.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RectQuad {
    pub(crate) kind: RectKind,
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
    pub(crate) fill: BrushSource,
    pub(crate) stroke: ShapeStroke,
}

/// What [`DrawQuadPayload::triangle`] lowers: three owner-local `points`
/// offset by `origin`, rounded by `radius`, with a solid fill and a stroke.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TriangleQuad {
    pub(crate) origin: Vec2,
    pub(crate) points: [Vec2; 3],
    pub(crate) radius: f32,
    pub(crate) fill: RgbaF16,
    pub(crate) stroke: ShapeStroke,
}

impl DrawQuadPayload {
    /// This draw with its alpha scaled by `by`, for
    /// [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)'s gate.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            fill: self.fill.faded(by),
            stroke: self.stroke.faded(by),
            ..self
        }
    }

    /// A rect of `q.kind` with `q.fill` and `q.stroke`. The brush is lowered
    /// here because the GPU lanes it fills are this type's. A
    /// [`RectKind::Windowed`] rect's `FillKind` carries the window bit, so the
    /// shader inverts the fill coverage and the composer's opaque-cover checks
    /// (`fill_kind == FillKind::SOLID`) do not treat its hole as an occluder.
    pub(crate) fn rect(q: RectQuad) -> Self {
        let RectQuad {
            kind,
            rect,
            corners,
            fill,
            stroke,
        } = q;
        // Stroke stays solid-only.
        let mut lanes = fill.gpu_fill();
        if kind == RectKind::Windowed {
            lanes.kind = lanes.kind.with_window();
        }
        Self {
            geometry: QuadGeometry::Rect { rect, corners },
            fill: lanes,
            stroke: stroke.normalized(),
            fill_axis: fill.fill_axis(),
        }
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
            geometry: QuadGeometry::Rect { rect, corners },
            fill: GpuFill {
                color: shadow.color,
                kind,
                lut_row: LutRow::FALLBACK,
            },
            // A shadow has no stroke; its edge is the blur.
            stroke: ShapeStroke::NONE,
            // The axis travels as the packed word; unpacking would round-trip identical bytes through f16.
            fill_axis: FillAxis::from(shadow.geometry_f16),
        })
    }

    /// A rounded triangle from `q`'s three owner-local points offset by its
    /// origin. Same tier and stroke normalization as [`Self::rect`].
    pub(crate) const fn triangle(q: TriangleQuad) -> Self {
        let TriangleQuad {
            origin,
            points: [a, b, c],
            radius,
            fill,
            stroke,
        } = q;
        Self {
            geometry: QuadGeometry::Triangle {
                origin,
                a,
                b,
                c,
                radius,
            },
            fill: GpuFill {
                color: fill,
                kind: FillKind::TRIANGLE,
                // The composer overwrites both reused lanes from the points.
                lut_row: LutRow::FALLBACK,
            },
            stroke: stroke.normalized(),
            fill_axis: FillAxis::ZERO,
        }
    }

    /// Paints nothing when the geometry covers no pixels, or neither fill nor
    /// stroke can put down a texel.
    ///
    /// Shadow parameters are not gated: a zero-σ drop shadow still paints a
    /// hard shifted rect, and `Shape::Shadow::is_noop` catches no-effect cases.
    ///
    /// A gradient fill is never a no-op here: [`BrushSource::gpu_fill`] sets
    /// its color lane to white (the atlas row supplies the color), and
    /// `Brush::is_noop` filters all-transparent stops before lowering.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.is_paint_empty() || (self.fill.is_noop() && self.stroke.is_noop())
    }

    /// Whether the geometry covers no pixels. A drop shadow covers its source
    /// grown by the halo. A triangle is `false`: its covering rect exists only
    /// after transform, and `TriangleShape::is_noop` filters degenerates.
    #[inline]
    fn is_paint_empty(&self) -> bool {
        match self.geometry {
            QuadGeometry::Rect { rect, .. } if self.fill.kind == FillKind::SHADOW_DROP => {
                let halo = ShadowGeometry::from_lanes(self.fill_axis.lanes()).halo();
                rect.inflated(halo).is_paint_empty()
            }
            QuadGeometry::Rect { rect, .. } => rect.is_paint_empty(),
            QuadGeometry::Triangle { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests;
