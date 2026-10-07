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
use crate::shape::paint::lowered_shadow::ShadowGeom;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::rect::RectKind;
use glam::Vec2;

/// The geometry half of a [`DrawQuadPayload`]. Rects, windowed rects and
/// shadows arrive as an already-resolved rect; a triangle's covering rect
/// only exists after its points are transformed, so it carries the points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum QuadGeom {
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
/// [`geom`](Self::geom) and the SDF `fill_kind` selects.
///
/// `fill_kind`'s low byte is the kind tag; bits 8..16 carry `Spread` for
/// gradients. `fill_lut_row` is the gradient atlas row, or
/// [`LutRow::FALLBACK`]. `fill_axis` is the gradient geometry, or for a
/// shadow `(0, 0, σ, spread)` (drop) / `(offset.x, offset.y, σ, spread)`
/// (inset) in logical px, scaled to physical by the composer. A triangle's
/// is overwritten by the composer.
///
/// `fill` is the solid colour (the tint for a shadow); for a gradient it
/// multiplies the atlas row's colour. `RgbaF16` saves 8 B per payload over
/// `RgbaF32`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawQuadPayload {
    pub(crate) geom: QuadGeom,
    pub(crate) fill: GpuFill,
    /// Normalized by [`ShapeStroke::normalized`], so `NONE` means no stroke exactly.
    pub(crate) stroke: ShapeStroke,
    /// The reused four-lane geometry slot: gradient axis, shadow `(offset, σ,
    /// spread)`, or unread for a triangle. Outside [`GpuFill`] because only
    /// this tier has one.
    pub(crate) fill_axis: FillAxis,
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

    /// A rounded rect with `fill` and `stroke`. The brush is lowered here
    /// because the GPU lanes it fills are this type's.
    pub(crate) fn rect(
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) -> Self {
        Self::rect_of_kind(RectKind::Rounded, rect, corners, fill, stroke)
    }

    /// [`Self::rect`] of either kind. A [`RectKind::Windowed`] rect's
    /// `FillKind` carries the window bit, so the shader inverts the fill
    /// coverage and the composer's opaque-cover checks
    /// (`fill_kind == FillKind::SOLID`) do not treat its hole as an occluder.
    pub(crate) fn rect_of_kind(
        kind: RectKind,
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) -> Self {
        // Stroke stays solid-only.
        let mut lanes = fill.gpu_fill();
        if kind == RectKind::Windowed {
            lanes.kind = lanes.kind.with_window();
        }
        Self {
            geom: QuadGeom::Rect { rect, corners },
            fill: lanes,
            stroke: stroke.normalized(),
            fill_axis: fill.fill_axis(),
        }
    }

    /// A shadow of the source `rect`: `color` the tint, `fill_kind`
    /// `SHADOW_DROP|SHADOW_INSET`, `fill_axis` `(offset.x, offset.y, σ,
    /// spread)`, scaled to physical px by the composer.
    pub(crate) const fn shadow(
        rect: Rect,
        corners: Corners,
        color: RgbaF16,
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
            // A shadow has no stroke; its edge is the blur.
            stroke: ShapeStroke::NONE,
            fill_axis,
        }
    }

    /// A rounded triangle from three owner-local `points` offset by `origin`.
    /// Same tier and stroke normalization as [`Self::rect`].
    pub(crate) const fn triangle(
        origin: Vec2,
        points: [Vec2; 3],
        fill: RgbaF16,
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
    /// its colour lane to white (the atlas row supplies the colour), and
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
        match self.geom {
            QuadGeom::Rect { rect, .. } if self.fill.kind == FillKind::SHADOW_DROP => {
                let halo = ShadowGeom::from_lanes(self.fill_axis.lanes()).halo();
                rect.inflated(halo).is_paint_empty()
            }
            QuadGeom::Rect { rect, .. } => rect.is_paint_empty(),
            QuadGeom::Triangle { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::packed::fill_axis::FillAxis;
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::primitives::paint::brush::gradient::Spread;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::primitives::paint::lut_row::LutRow;
    use crate::primitives::paint::stroke::Stroke;
    use crate::renderer::frontend::payload::brush_source::BrushSource;
    use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
    use crate::renderer::frontend::payload::draw_quad_payload::QuadGeom;
    use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
    use crate::shape::paint::shape_stroke::ShapeStroke;
    use glam::Vec2;

    /// Every quad-tier constructor normalizes the stroke
    /// ([`ShapeStroke::normalized`]): a transparent, zero-width or NaN-width
    /// stroke becomes [`ShapeStroke::NONE`]; others pass verbatim.
    ///
    /// NaN normalizes away like any non-painting width: catching it loudly is
    /// the `has_nan` screen's job at `Shapes::add`, so here it fails safe,
    /// identically for every shape.
    ///
    /// Pins the exact normalized stroke per case, and that
    /// [`DrawQuadPayload::rect`] and [`DrawQuadPayload::triangle`] produce
    /// bit-identical strokes.
    #[test]
    fn quad_stroke_normalization_is_shared_by_rect_and_triangle() {
        let fill = RgbaF32::srgb(1.0, 0.0, 0.0);
        let green = RgbaF32::srgb(0.0, 1.0, 0.0);
        let cases: [(&str, ShapeStroke, bool); 4] = [
            (
                "transparent_color",
                Stroke::new(RgbaF32::TRANSPARENT, 3.0).into(),
                true,
            ),
            ("zero_width", Stroke::new(green, 0.0).into(), true),
            ("nan_width", Stroke::new(green, f32::NAN).into(), true),
            ("live", Stroke::new(green, 3.0).into(), false),
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
                assert_eq!(tp.stroke.color, RgbaF16::TRANSPARENT, "case {label}");
                assert_eq!(tp.stroke.width, 0.0, "case {label}");
            } else {
                assert_eq!(tp.stroke.color, RgbaF16::from(green), "case {label}");
            }
        }
    }

    /// A fade reaches a solid fill and a gradient fill through the one alpha
    /// lane. A solid scales its real alpha (half of `0.8` is `0.4`); a
    /// gradient's lane multiplies the sample, starting white, so a half fade
    /// gives alpha `0.5` with RGB at one. Fading to zero makes either a no-op,
    /// the stroke scales either way, and `faded(1.0)` changes nothing.
    #[test]
    fn a_fade_reaches_the_solid_alpha_and_the_gradients_multiplier() {
        let stroke = ShapeStroke {
            width: 2.0,
            color: RgbaF16::new(1.0, 1.0, 1.0, 1.0),
        };
        let quad = |fill| {
            DrawQuadPayload::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Corners::ZERO, fill, stroke)
        };

        let solid = quad(BrushSource::Solid(RgbaF16::new(0.25, 0.5, 0.75, 0.8)));
        assert_eq!(solid.faded(1.0), solid);
        let faded = solid.faded(0.5);
        // 0.8 packs to f16 as 1638 steps of 2^-11, and halving it is exact.
        assert_eq!(faded.fill.color.unpack().a, 1638.0 / 4096.0);
        assert_eq!(faded.stroke.color.unpack().a, 0.5);

        let gradient = quad(BrushSource::Gradient(ResolvedGradient {
            axis: FillAxis::ZERO,
            lut_row: LutRow::FALLBACK,
            kind: FillKind::linear(Spread::Pad),
        }));
        assert_eq!(
            gradient.fill.color,
            RgbaF16::WHITE,
            "an unfaded gradient multiplies its sample by one",
        );
        let faded = gradient.faded(0.5);
        let multiplier = faded.fill.color.unpack();
        assert_eq!((multiplier.r, multiplier.g, multiplier.b), (1.0, 1.0, 1.0));
        assert_eq!(multiplier.a, 0.5);
        assert_eq!(faded.fill.lut_row, gradient.fill.lut_row);

        assert!(!solid.fill.is_noop() && !gradient.fill.is_noop());
        assert!(solid.fill.faded(0.0).is_noop());
        assert!(
            gradient.fill.faded(0.0).is_noop(),
            "a gradient faded out paints nothing"
        );
    }
}
