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
use crate::renderer::frontend::payload::draw_quad_payload::{
    DrawQuadPayload, QuadGeometry, RectQuad, TriangleQuad,
};
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::rect::RectKind;
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
        let rp = DrawQuadPayload::rect(RectQuad {
            kind: RectKind::Rounded,
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            corners: Corners::ZERO,
            fill: BrushSource::Solid(fill.into()),
            stroke,
        });
        assert!(
            matches!(rp.geometry, QuadGeometry::Rect { .. }),
            "case {label}: rect must carry rect geometry",
        );

        let tp = DrawQuadPayload::triangle(TriangleQuad {
            origin: Vec2::ZERO,
            points: [
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(5.0, 8.0),
            ],
            radius: 0.0,
            fill: fill.into(),
            stroke,
        });
        assert!(
            matches!(tp.geometry, QuadGeometry::Triangle { .. }),
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
        DrawQuadPayload::rect(RectQuad {
            kind: RectKind::Rounded,
            rect: Rect::new(0.0, 0.0, 8.0, 8.0),
            corners: Corners::ZERO,
            fill,
            stroke,
        })
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
