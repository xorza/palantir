//! The quad-tier draw: rounded rects, windowed rects, box-shadows and
//! rounded triangles, which all lower to one `Quad` instance.

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::color::ColorF16;
use crate::primitives::corners::Corners;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::lut_row::LutRow;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::scene::shapes::paint::ShapeStroke;
use glam::Vec2;

/// The geometry half of a [`DrawQuadPayload`] — everything the composer
/// needs to derive the instance's physical rect and its two reused
/// lanes. Rectangles, windowed rectangles, and box-shadows all arrive
/// as an already-resolved rect, so they share one variant; a triangle
/// is the one shape whose covering rect only exists *after* its points
/// are transformed, so it carries the points instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum QuadGeom {
    /// A logical-px paint rect + corner radii. For a drop shadow the
    /// rect is the offset source inflated by `3σ + max(spread, 0)`; for
    /// an inset shadow it is the source, and `corners` carries the
    /// *source* shape's radii either way.
    Rect { rect: Rect, corners: Corners },
    /// Owner-local corner points and corner rounding. The composer
    /// folds `origin` (the owner-rect top-left) + the active
    /// push-transform before scaling to physical px, then derives the
    /// covering AABB (the points inflated by `radius + AA fringe`) and
    /// packs the physical points into the `corners` / `fill_axis` lanes.
    Triangle {
        origin: Vec2,
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
    },
}

impl QuadGeom {
    /// Whether this geometry covers no pixels on its own. A triangle
    /// always answers `false`: its covering rect doesn't exist until the
    /// composer transforms the points, and degenerate corners are
    /// already filtered at the authoring boundary by
    /// `TriangleShape::is_noop`.
    #[inline]
    fn is_paint_empty(&self) -> bool {
        match self {
            Self::Rect { rect, .. } => rect.is_paint_empty(),
            Self::Triangle { .. } => false,
        }
    }
}

/// One quad-tier draw: a rounded rect, a windowed rect, a box-shadow,
/// or a rounded triangle. All four lower to a single `Quad` instance on
/// the one quad pipeline, so they share this payload and differ only in
/// [`geom`](Self::geom) and which SDF `fill_kind` selects.
///
/// `fill_kind`'s low byte is the kind tag; bits 8..16 carry `Spread` for
/// gradient variants. `fill_lut_row` is the pre-registered gradient
/// atlas row (set at shape lowering time), or [`LutRow::FALLBACK`] for
/// everything else. `fill_axis` carries gradient geometry packed at
/// lowering, or — for a shadow — `(0, 0, σ, spread)` for drops and
/// `(offset.x, offset.y, σ, spread)` for insets in logical px, which the
/// composer scales to physical px so the shader's `local` coords line
/// up. A triangle's `fill_axis` is unread; the composer overwrites both
/// reused lanes from the transformed points.
///
/// `fill: ColorF16` is the solid colour when `kind == SOLID` (and the
/// tint when it's a shadow); zeroed for gradients, where the atlas row
/// supplies the colour. Storing as `ColorF16` (8 B linear-RGB) vs. 16 B
/// `Color` saves 8 B per payload — the composer decodes via
/// `Color::from(f16)` at `Quad` write time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawQuadPayload {
    pub(crate) geom: QuadGeom,
    pub(crate) fill: GpuFill,
    /// Normalized by [`ShapeStroke::normalized`] on the way in, so
    /// [`ShapeStroke::NONE`] here means "no stroke" exactly.
    pub(crate) stroke: ShapeStroke,
    /// The reused four-lane geometry slot: gradient axis for a gradient
    /// fill, `(offset, σ, spread)` for a shadow, unread for a triangle
    /// (the composer overwrites it from the transformed points). Not part
    /// of [`GpuFill`] because only this tier has one, and because a
    /// shadow's lanes are not an axis at all.
    pub(crate) fill_axis: FillAxis,
}

impl DrawQuadPayload {
    /// A rounded rect with `fill` and `stroke`.
    ///
    /// The brush is lowered here rather than at the call site because
    /// the GPU lanes it fills — colour, kind, LUT row, axis — are this
    /// type's, and a caller assembling them by hand is a caller that
    /// can get the gradient case wrong.
    pub(crate) fn rect(
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) -> Self {
        Self::rect_impl(rect, corners, fill, stroke, false)
    }

    /// Windowed sibling of [`Self::rect`]: same payload, but the
    /// `FillKind` carries the window bit, so the shader inverts the fill
    /// coverage (fill outside the rounded boundary, transparent window
    /// inside the stroke). The bit also keeps the composer's opaque-cover
    /// checks (`fill_kind == FillKind::SOLID`) from treating the quad as
    /// an occluder — its interior is a hole.
    pub(crate) fn rect_window(
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) -> Self {
        Self::rect_impl(rect, corners, fill, stroke, true)
    }

    fn rect_impl(
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
        window: bool,
    ) -> Self {
        // Stroke stays solid-only — gradient strokes are a non-goal.
        let mut lanes = fill.gpu_fill();
        if window {
            lanes.kind = lanes.kind.with_window();
        }
        Self {
            geom: QuadGeom::Rect { rect, corners },
            fill: lanes,
            stroke: stroke.normalized(),
            fill_axis: fill.fill_axis(),
        }
    }

    /// A shadow. For a drop shadow, `rect` is the offset source inflated
    /// by `3σ + max(spread, 0)`; for an inset shadow it is the source
    /// rect. `corners` is the source shape's corner radii, `color` the
    /// shadow tint, `fill_kind` `FillKind::SHADOW_DROP|SHADOW_INSET`.
    /// Drop shadows carry `(0, 0, σ, spread)` in `fill_axis`; inset
    /// shadows carry `(offset.x, offset.y, σ, spread)`. The composer
    /// scales the logical-px lanes to physical px on emit.
    pub(crate) fn shadow(
        rect: Rect,
        corners: Corners,
        color: ColorF16,
        fill_kind: FillKind,
        fill_axis: FillAxis,
    ) -> Self {
        Self {
            geom: QuadGeom::Rect { rect, corners },
            fill: GpuFill {
                color,
                kind: fill_kind,
                lut_row: LutRow::FALLBACK,
            },
            // A shadow has no stroke; its whole edge is the blur.
            stroke: ShapeStroke::NONE,
            fill_axis,
        }
    }

    /// A rounded triangle from three owner-local `points` offset by
    /// `origin`. Same quad tier as [`Self::rect`], down to the shared
    /// stroke normalization — only the geometry and the SDF the
    /// `FillKind` selects differ.
    pub(crate) fn triangle(
        origin: Vec2,
        points: [Vec2; 3],
        fill: ColorF16,
        radius: f32,
        stroke: ShapeStroke,
    ) -> Self {
        let [a, b, c] = points;
        Self {
            geom: QuadGeom::Triangle {
                origin,
                a,
                b,
                c,
                radius,
            },
            fill: GpuFill {
                color: fill,
                kind: FillKind::TRIANGLE,
                // The composer overwrites both reused lanes from the
                // transformed points, so neither is read from here.
                lut_row: LutRow::FALLBACK,
            },
            stroke: stroke.normalized(),
            fill_axis: FillAxis::ZERO,
        }
    }

    /// Paints nothing when the geometry covers no pixels, or when
    /// neither the fill nor the stroke can put down a texel.
    ///
    /// Shadow parameters themselves (`fill_axis`) are not gated: a
    /// zero-σ drop shadow still paints a hard-edged shifted rect, and
    /// the `Shape::Shadow::is_noop` authoring boundary is what catches
    /// the "no visible effect" cases.
    ///
    /// A gradient fill is never a no-op here. [`BrushSource::gpu_fill`]
    /// zeroes its colour lane — the atlas row supplies the colour — so
    /// judging it by that lane would read every gradient as transparent
    /// and drop the draw. Nor does it need to be: `Brush::is_noop`
    /// filters the all-transparent-stops case *before* lowering, and one
    /// slipping past that gate would paint a useless transparent quad
    /// whose alpha blend produces nothing visible.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.geom.is_paint_empty() || (self.fill.is_noop() && self.stroke.is_noop())
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::{Color, ColorF16};
    use crate::primitives::corners::Corners;
    use crate::primitives::fill_kind::FillKind;
    use crate::primitives::rect::Rect;
    use crate::primitives::stroke::Stroke;
    use crate::renderer::frontend::payload::brush_source::BrushSource;
    use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
    use crate::renderer::frontend::payload::draw_quad_payload::QuadGeom;
    use crate::scene::shapes::paint::ShapeStroke;
    use glam::Vec2;

    /// Every quad-tier constructor runs one stroke normalization
    /// ([`ShapeStroke::normalized`]): a noop stroke — transparent
    /// colour, zero width, or a NaN width — lands in the payload as
    /// [`ShapeStroke::NONE`]; anything else passes through verbatim.
    ///
    /// The NaN row is the interesting one. It normalizes away like any
    /// other non-painting width, which is deliberate: catching a NaN
    /// *loudly* is the `has_nan` screen's job at `Shapes::add`, the
    /// authoring boundary, so by the time a value reaches here the useful
    /// behaviour is to fail safe — and to do it identically for every
    /// shape, rather than per-path (a rect forwarding NaN to the GPU
    /// while a triangle scrubs it).
    ///
    /// The table pins both halves: the exact normalized stroke per case,
    /// **and** that [`DrawQuadPayload::rect`] and
    /// [`DrawQuadPayload::triangle`] produce bit-identical stroke fields
    /// — the regression guard against either growing its own copy again.
    #[test]
    fn quad_stroke_normalization_is_shared_by_rect_and_triangle() {
        let fill = Color::rgb(1.0, 0.0, 0.0);
        let green = Color::rgb(0.0, 1.0, 0.0);
        let cases: [(&str, ShapeStroke, bool); 4] = [
            (
                "transparent_color",
                Stroke::solid(Color::TRANSPARENT, 3.0).into(),
                true,
            ),
            ("zero_width", Stroke::solid(green, 0.0).into(), true),
            ("nan_width", Stroke::solid(green, f32::NAN).into(), true),
            ("live", Stroke::solid(green, 3.0).into(), false),
        ];
        for (label, stroke, expect_normalized) in cases {
            let rp = DrawQuadPayload::rect(
                Rect::new(0.0, 0.0, 10.0, 10.0),
                Corners::ZERO,
                BrushSource::Solid(fill.into()),
                stroke,
            );
            assert!(
                matches!(rp.geom, QuadGeom::Rect { .. }),
                "case {label}: rect must carry rect geometry",
            );

            let tp = DrawQuadPayload::triangle(
                Vec2::ZERO,
                [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 0.0),
                    Vec2::new(5.0, 8.0),
                ],
                fill.into(),
                0.0,
                stroke,
            );
            assert!(
                matches!(tp.geom, QuadGeom::Triangle { .. }),
                "case {label}: triangle must carry triangle geometry",
            );
            assert_eq!(tp.fill.kind, FillKind::TRIANGLE, "case {label}");

            assert_eq!(tp.stroke.color, rp.stroke.color, "case {label}");
            assert_eq!(
                tp.stroke.width.to_bits(),
                rp.stroke.width.to_bits(),
                "case {label}",
            );
            if expect_normalized {
                assert_eq!(tp.stroke.color, ColorF16::TRANSPARENT, "case {label}");
                assert_eq!(tp.stroke.width, 0.0, "case {label}");
            } else {
                assert_eq!(tp.stroke.color, ColorF16::from(green), "case {label}");
            }
        }
    }
}
