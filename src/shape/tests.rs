use crate::layout::types::align::Align;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::brush::{Brush, CurveBrush};
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::shape::Shape;
use crate::shape::rect::RectKind;
use crate::shape::sealed::LowerShape as _;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use crate::text::{FontFamily, FontWeight};
use glam::Vec2;

#[test]
fn triangle_noop_rejects_scale_relative_zero_area_without_winding_bias() {
    #[derive(Clone, Copy, Debug)]
    struct Case {
        label: &'static str,
        a: Vec2,
        b: Vec2,
        c: Vec2,
        expected_noop: bool,
    }

    let cases = [
        Case {
            label: "counter_clockwise",
            a: Vec2::ZERO,
            b: Vec2::new(100.0, 0.0),
            c: Vec2::new(0.0, 100.0),
            expected_noop: false,
        },
        Case {
            label: "clockwise",
            a: Vec2::ZERO,
            b: Vec2::new(0.0, 100.0),
            c: Vec2::new(100.0, 0.0),
            expected_noop: false,
        },
        Case {
            label: "collinear",
            a: Vec2::ZERO,
            b: Vec2::new(40.0, 40.0),
            c: Vec2::new(100.0, 100.0),
            expected_noop: true,
        },
        Case {
            label: "repeated_vertex",
            a: Vec2::new(10.0, 20.0),
            b: Vec2::new(10.0, 20.0),
            c: Vec2::new(100.0, 100.0),
            expected_noop: true,
        },
        Case {
            label: "near_degenerate_unit_scale",
            a: Vec2::ZERO,
            b: Vec2::new(1.0, 0.0),
            c: Vec2::new(1.0, 0.00005),
            expected_noop: true,
        },
        Case {
            label: "near_degenerate_hundred_scale",
            a: Vec2::ZERO,
            b: Vec2::new(100.0, 0.0),
            c: Vec2::new(100.0, 0.005),
            expected_noop: true,
        },
        Case {
            label: "above_threshold_unit_scale",
            a: Vec2::ZERO,
            b: Vec2::new(1.0, 0.0),
            c: Vec2::new(1.0, 0.0002),
            expected_noop: false,
        },
        Case {
            label: "above_threshold_hundred_scale",
            a: Vec2::ZERO,
            b: Vec2::new(100.0, 0.0),
            c: Vec2::new(100.0, 0.02),
            expected_noop: false,
        },
    ];

    for case in cases {
        let shape = Shape::triangle(case.a, case.b, case.c).fill(Color::WHITE);
        assert_eq!(shape.fill, Color::WHITE, "case: {}", case.label);
        assert_eq!(shape.is_noop(), case.expected_noop, "case: {}", case.label);
    }
}

/// The constructors return the concrete shape they name, so "did the
/// builder survive erasure into a `Shape` variant" is no longer a
/// question the type system leaves open. What is still worth pinning is
/// that each builder writes the field it advertises.
#[test]
fn typed_builders_set_the_fields_they_name() {
    let rect = Rect::new(1.0, 2.0, 30.0, 40.0);
    let gradient = LinearGradient::two_stop(0.25, Color::BLACK, Color::WHITE);
    let stroke = Stroke::solid(Color::WHITE, 2.0);
    let rect_shape = Shape::windowed_rect(rect)
        .fill(gradient.clone())
        .stroke(stroke)
        .corners(6.0);
    assert!(matches!(rect_shape.kind, RectKind::Windowed));
    assert_eq!(rect_shape.local_rect, Some(rect));
    assert_eq!(rect_shape.fill, Brush::Linear(gradient));
    assert_eq!(rect_shape.stroke, stroke);
    assert_eq!(rect_shape.corners.as_array(), [6.0; 4]);

    let mesh = Mesh::new();
    let tint = ColorU8::linear_rgb(10, 20, 30);
    let mesh_shape = Shape::mesh(&mesh).at(rect).tint(tint);
    assert!(std::ptr::eq(mesh_shape.mesh, &mesh));
    assert_eq!(mesh_shape.local_rect, Some(rect));
    assert_eq!(mesh_shape.tint, tint.into());
}

#[test]
fn text_noop_rejects_invalid_metrics() {
    use crate::primitives::approx::EPS;

    let store = RecordStore::default();
    let cases = [
        ("valid", 16.0, 19.2, false),
        ("zero font", 0.0, 19.2, true),
        ("negative font", -1.0, 19.2, true),
        ("sub-epsilon font", EPS * 0.5, 19.2, true),
        ("epsilon font", EPS, 19.2, true),
        ("NaN font", f32::NAN, 19.2, true),
        ("infinite font", f32::INFINITY, 19.2, true),
        ("zero line height", 16.0, 0.0, true),
        ("negative line height", 16.0, -1.0, true),
        ("sub-epsilon line height", 16.0, EPS * 0.5, true),
        ("epsilon line height", 16.0, EPS, true),
        ("NaN line height", 16.0, f32::NAN, true),
        ("infinite line height", 16.0, f32::INFINITY, true),
    ];

    // `local_origin` is the one Text scalar `GlyphFont::metrics_valid`
    // does not cover, and it is the NaN screen's rather than the no-op
    // screen's: an origin that is not a number is not a reason the run
    // paints nothing. Both run before lowering, which is what keeps the
    // interned bytes out of the arena either way.
    for (label, local_origin, expected_nan) in [
        ("no origin", None, false),
        ("finite origin", Some(Vec2::new(1.0, 2.0)), false),
        ("NaN origin x", Some(Vec2::new(f32::NAN, 2.0)), true),
        ("NaN origin y", Some(Vec2::new(1.0, f32::NAN)), true),
    ] {
        let shape = Shape::text(
            store.intern_str("visible"),
            GlyphFont {
                line_height_px: 19.2,
                ..GlyphFont::new(16.0)
            },
        )
        .color(Color::WHITE);
        let shape = match local_origin {
            Some(origin) => shape.at(origin),
            None => shape,
        };
        assert_eq!(shape.has_nan(), expected_nan, "{label}");
        assert!(
            !shape.is_noop(),
            "{label}: a visible run does not stop painting over its origin",
        );
    }

    for (label, font_size_px, line_height_px, expected_noop) in cases {
        let shape = Shape::text(
            store.intern_str("visible"),
            GlyphFont {
                line_height_px,
                ..GlyphFont::new(font_size_px)
            },
        )
        .color(Color::WHITE)
        .wrap(TextWrap::SingleLine)
        .align(Align::TOP_LEFT)
        .family(FontFamily::Sans)
        .weight(FontWeight::Regular);
        assert_eq!(shape.is_noop(), expected_noop, "{label}");
    }
}

#[test]
fn curve_brush_conversions_preserve_supported_paints_and_noop_state() {
    #[derive(Debug)]
    struct Case {
        label: &'static str,
        brush: CurveBrush,
        expected_noop: bool,
    }

    let visible_gradient = LinearGradient::two_stop(0.0, Color::TRANSPARENT, Color::WHITE);
    let transparent_gradient =
        LinearGradient::two_stop(0.0, Color::TRANSPARENT, Color::TRANSPARENT);
    let cases = [
        Case {
            label: "transparent_solid",
            brush: Color::TRANSPARENT.into(),
            expected_noop: true,
        },
        Case {
            label: "visible_solid",
            brush: ColorU8::WHITE.into(),
            expected_noop: false,
        },
        Case {
            label: "transparent_linear",
            brush: transparent_gradient.into(),
            expected_noop: true,
        },
        Case {
            label: "visible_linear",
            brush: visible_gradient.into(),
            expected_noop: false,
        },
    ];

    for case in cases {
        let shape = Shape::line(Vec2::ZERO, Vec2::X, 1.0).brush(case.brush.clone());
        assert_eq!(
            case.brush.as_brush().is_noop(),
            case.expected_noop,
            "case: {}",
            case.label,
        );
        assert_eq!(shape.is_noop(), case.expected_noop, "case: {}", case.label);
    }
}
