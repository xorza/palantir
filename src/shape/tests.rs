use crate::layout::types::align::Align;
use crate::primitives::brush::gradient::linear::LinearGradient;
use crate::primitives::brush::{Brush, CurveBrush};
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::shape::Shape;
use crate::shape::rect::RectKind;
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
        let shape: Shape<'_> = Shape::triangle(case.a, case.b, case.c)
            .fill(Color::WHITE)
            .into();
        let Shape::Triangle(triangle) = &shape else {
            panic!("Shape::triangle must construct a triangle shape");
        };
        assert_eq!(triangle.fill, Color::WHITE, "case: {}", case.label);
        assert_eq!(shape.is_noop(), case.expected_noop, "case: {}", case.label);
    }
}

#[test]
fn typed_builders_preserve_convertible_variant_configuration() {
    let rect = Rect::new(1.0, 2.0, 30.0, 40.0);
    let gradient = LinearGradient::two_stop(0.25, Color::BLACK, Color::WHITE);
    let stroke = Stroke::solid(Color::WHITE, 2.0);
    let rect_shape: Shape<'_> = Shape::windowed_rect(rect)
        .fill(gradient.clone())
        .stroke(stroke)
        .corners(6.0)
        .into();
    let Shape::Rect(rect_shape) = rect_shape else {
        panic!("Shape::windowed_rect must construct a rectangle shape");
    };
    assert!(matches!(rect_shape.kind, RectKind::Windowed));
    assert_eq!(rect_shape.local_rect, Some(rect));
    assert_eq!(rect_shape.fill, Brush::Linear(gradient));
    assert_eq!(rect_shape.stroke, stroke);
    assert_eq!(rect_shape.corners.as_array(), [6.0; 4]);

    let mesh = Mesh::new();
    let tint = ColorU8::rgb(10, 20, 30);
    let mesh_shape: Shape<'_> = Shape::mesh(&mesh).at(rect).tint(tint).into();
    let Shape::Mesh(mesh_shape) = mesh_shape else {
        panic!("Shape::mesh must construct a mesh shape");
    };
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

    for (label, font_size_px, line_height_px, expected_noop) in cases {
        let shape = Shape::Text {
            local_origin: None,
            text: store.intern_str("visible"),
            color: Color::WHITE,
            font_size_px,
            line_height_px,
            wrap: TextWrap::SingleLine,
            align: Align::TOP_LEFT,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
        };
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
        let shape: Shape<'_> = Shape::line(Vec2::ZERO, Vec2::X, 1.0)
            .brush(case.brush.clone())
            .into();
        assert_eq!(
            case.brush.is_noop(),
            case.expected_noop,
            "case: {}",
            case.label,
        );
        assert_eq!(shape.is_noop(), case.expected_noop, "case: {}", case.label);
    }
}

/// Every authoring shape screens its own inputs for NaN, and the
/// `Shape` dispatch reaches all of them. A missed field is a silent
/// hole: `Shapes::add`'s assert is the only place a NaN is named before
/// it disappears into a bbox.
///
/// Each case plants one NaN in one field and pairs it with the same
/// shape built clean, so a `true` can only come from the planted value
/// — a check that returned `true` unconditionally would fail the
/// negative half.
#[test]
fn every_shape_reports_a_nan_in_any_of_its_inputs() {
    use crate::primitives::nan::NanCheck;
    use crate::primitives::shadow::Shadow;
    use crate::shape::polyline::PolylineColors;

    const N: f32 = f32::NAN;
    let store = RecordStore::default();
    let nan_pt = Vec2::new(1.0, N);
    let nan_rect = Rect::new(0.0, N, 4.0, 4.0);
    let nan_color = Color::rgba(0.0, N, 0.0, 1.0);
    let ok_rect = Rect::new(0.0, 0.0, 4.0, 4.0);
    let pts_ok = [Vec2::ZERO, Vec2::new(4.0, 0.0)];
    let pts_nan = [Vec2::ZERO, nan_pt];
    let cols_ok = [Color::WHITE, Color::WHITE];
    let cols_nan = [Color::WHITE, nan_color];
    let one_vertex = |pos| {
        let mut mesh = Mesh::new();
        mesh.vertex(pos, ColorU8::WHITE);
        mesh
    };
    let mesh_ok = one_vertex(Vec2::ZERO);
    let mesh_nan = one_vertex(nan_pt);

    // (label, shape carrying one NaN, the same shape built clean)
    let cases: [(&str, Shape<'_>, Shape<'_>); 12] = [
        (
            "rect_local_rect",
            Shape::rect(nan_rect).into(),
            Shape::rect(ok_rect).into(),
        ),
        (
            "rect_corners",
            Shape::rect(ok_rect).corners(N).into(),
            Shape::rect(ok_rect).corners(2.0).into(),
        ),
        (
            "rect_fill",
            Shape::rect(ok_rect).fill(nan_color).into(),
            Shape::rect(ok_rect).fill(Color::WHITE).into(),
        ),
        (
            "rect_stroke_width",
            Shape::rect(ok_rect)
                .stroke(Stroke::solid(Color::WHITE, N))
                .into(),
            Shape::rect(ok_rect)
                .stroke(Stroke::solid(Color::WHITE, 1.0))
                .into(),
        ),
        (
            "triangle_corner",
            Shape::triangle(Vec2::ZERO, Vec2::X, nan_pt).into(),
            Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::Y).into(),
        ),
        (
            "triangle_radius",
            Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::Y)
                .radius(N)
                .into(),
            Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::Y)
                .radius(1.0_f32)
                .into(),
        ),
        (
            "curve_width",
            Shape::line(Vec2::ZERO, Vec2::X, N).into(),
            Shape::line(Vec2::ZERO, Vec2::X, 1.0).into(),
        ),
        (
            "curve_arc_sweep",
            Shape::arc(Vec2::ZERO, 4.0, 0.0, N, 1.0).into(),
            Shape::arc(Vec2::ZERO, 4.0, 0.0, 1.0, 1.0).into(),
        ),
        (
            "polyline_points",
            Shape::polyline(&pts_nan, PolylineColors::PerPoint(&cols_ok), 1.0).into(),
            Shape::polyline(&pts_ok, PolylineColors::PerPoint(&cols_ok), 1.0).into(),
        ),
        (
            "polyline_colors",
            Shape::polyline(&pts_ok, PolylineColors::PerPoint(&cols_nan), 1.0).into(),
            Shape::polyline(&pts_ok, PolylineColors::PerPoint(&cols_ok), 1.0).into(),
        ),
        (
            "shadow_blur",
            Shape::shadow(Shadow {
                blur: N,
                ..Shadow::default()
            })
            .into(),
            Shape::shadow(Shadow::default()).into(),
        ),
        (
            "mesh_vertex",
            Shape::mesh(&mesh_nan).into(),
            Shape::mesh(&mesh_ok).into(),
        ),
    ];

    for (label, tainted, clean) in cases {
        assert!(tainted.has_nan(), "case {label}: NaN went unnoticed");
        assert!(
            !clean.has_nan(),
            "case {label}: clean shape reported a NaN it doesn't have",
        );
    }

    // Text rides `Shape`'s own arm rather than a payload struct, and
    // `Image`'s only scalars outside the shared ones live in `Tile`.
    let text = |origin, size, color| Shape::Text {
        local_origin: origin,
        text: store.intern_str("visible"),
        color,
        font_size_px: size,
        line_height_px: 14.0,
        wrap: TextWrap::SingleLine,
        align: Align::CENTER,
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
    };
    assert!(text(Some(nan_pt), 12.0, Color::WHITE).has_nan(), "origin");
    assert!(text(None, N, Color::WHITE).has_nan(), "font size");
    assert!(text(None, 12.0, nan_color).has_nan(), "colour");
    assert!(!text(None, 12.0, Color::WHITE).has_nan(), "clean text");
}
