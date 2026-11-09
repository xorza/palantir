use crate::effect::effect_registry::EffectRegistry;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::quadratic_bezier::QuadraticBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::Align;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::line_style::LineCap;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::reduction::Reductions;
use crate::shape::Shape;
use crate::shape::geometry::arc_geometry::ArcGeometry;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::placed_stroke::PlacedStroke;
use crate::shape::geometry::point_placement::PointPlacement;
use crate::shape::geometry::triangle_geometry::PlacedTriangle;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::record_store::Resolve;
use crate::shape::sealed::DrawWith;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::ptr;

/// The record `store` records for a shape and its paint, under full reductions and no effects.
fn lower_in<S: DrawWith<P>, P>(
    store: &mut RecordStore,
    (shape, paint): (S, P),
) -> Option<ShapeRecord> {
    let effects = EffectRegistry::default();
    let resolve = Resolve {
        effects: &effects,
        reductions: Reductions::FULL,
    };
    store.record(shape.draw(&paint), resolve)
}

/// The record a shape and its paint lower to in a store of its own.
fn lower<S: DrawWith<P>, P>(pair: (S, P)) -> Option<ShapeRecord> {
    lower_in(&mut RecordStore::default(), pair)
}

#[test]
fn triangle_noop_rejects_scale_relative_zero_area_without_winding_bias() {
    #[derive(Clone, Copy, Debug)]
    struct Case {
        label: &'static str,
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
        expected_noop: bool,
    }

    let cases = [
        Case {
            label: "counter_clockwise",
            a: Vec2::ZERO,
            b: Vec2::new(100.0, 0.0),
            c: Vec2::new(0.0, 100.0),
            radius: 0.0,
            expected_noop: false,
        },
        Case {
            label: "clockwise",
            a: Vec2::ZERO,
            b: Vec2::new(0.0, 100.0),
            c: Vec2::new(100.0, 0.0),
            radius: 0.0,
            expected_noop: false,
        },
        Case {
            label: "collinear",
            a: Vec2::ZERO,
            b: Vec2::new(40.0, 40.0),
            c: Vec2::new(100.0, 100.0),
            radius: 0.0,
            expected_noop: true,
        },
        Case {
            label: "repeated_vertex",
            a: Vec2::new(10.0, 20.0),
            b: Vec2::new(10.0, 20.0),
            c: Vec2::new(100.0, 100.0),
            radius: 0.0,
            expected_noop: true,
        },
        Case {
            label: "near_degenerate_unit_scale",
            a: Vec2::ZERO,
            b: Vec2::new(1.0, 0.0),
            c: Vec2::new(1.0, 0.00005),
            radius: 0.0,
            expected_noop: true,
        },
        Case {
            label: "near_degenerate_hundred_scale",
            a: Vec2::ZERO,
            b: Vec2::new(100.0, 0.0),
            c: Vec2::new(100.0, 0.005),
            radius: 0.0,
            expected_noop: true,
        },
        Case {
            label: "above_threshold_unit_scale",
            a: Vec2::ZERO,
            b: Vec2::new(1.0, 0.0),
            c: Vec2::new(1.0, 0.0002),
            radius: 0.0,
            expected_noop: false,
        },
        Case {
            label: "above_threshold_hundred_scale",
            a: Vec2::ZERO,
            b: Vec2::new(100.0, 0.0),
            c: Vec2::new(100.0, 0.02),
            radius: 0.0,
            expected_noop: false,
        },
        Case {
            label: "collinear_rounded",
            a: Vec2::ZERO,
            b: Vec2::new(40.0, 40.0),
            c: Vec2::new(100.0, 100.0),
            radius: 3.0,
            expected_noop: true,
        },
        Case {
            label: "near_degenerate_hundred_scale_rounded",
            a: Vec2::ZERO,
            b: Vec2::new(100.0, 0.0),
            c: Vec2::new(50.0, 0.004),
            radius: 3.0,
            expected_noop: true,
        },
        Case {
            label: "point",
            a: Vec2::new(10.0, 10.0),
            b: Vec2::new(10.0, 10.0),
            c: Vec2::new(10.0, 10.0),
            radius: 0.0,
            expected_noop: true,
        },
        Case {
            label: "point_rounded",
            a: Vec2::new(10.0, 10.0),
            b: Vec2::new(10.0, 10.0),
            c: Vec2::new(10.0, 10.0),
            radius: 3.0,
            expected_noop: true,
        },
    ];

    for case in cases {
        let shape = Shape::triangle(case.a, case.b, case.c).radius(case.radius);
        assert_eq!(
            lower((shape, RgbaF32::WHITE)).is_none(),
            case.expected_noop,
            "case: {}",
            case.label
        );
    }
}

/// Rounding moves each edge in by the radius and dilates back by it, so the edges stay put.
///
/// The 30-40-50 right triangle has perimeter 120, incenter `(30·40, 40·30) / 120 = (10, 10)` and
/// inradius `1200 / 120 = 10`. Radius 5 halves each corner's way to the incenter: `(5, 5)`,
/// `(20, 5)`, `(5, 25)`, whose edges `y = 5`, `x = 5` and `4x + 3y = 95` lie 5 inside the sharp
/// `y = 0`, `x = 0` and `4x + 3y = 120`. Radius 25 clamps to the inradius: all three centers sit
/// on the incenter, and the shape is the incircle.
#[test]
fn triangle_rounding_keeps_the_edges() {
    let mut store = RecordStore::default();
    let cases = [
        (
            0.0,
            [Vec2::ZERO, Vec2::new(30.0, 0.0), Vec2::new(0.0, 40.0)],
            0.0,
            Rect::new(0.0, 0.0, 30.0, 40.0),
        ),
        (
            5.0,
            [
                Vec2::new(5.0, 5.0),
                Vec2::new(20.0, 5.0),
                Vec2::new(5.0, 25.0),
            ],
            5.0,
            Rect::new(0.0, 0.0, 25.0, 30.0),
        ),
        (
            25.0,
            [Vec2::splat(10.0); 3],
            10.0,
            Rect::new(0.0, 0.0, 20.0, 20.0),
        ),
    ];
    for (radius, centers, clamped, expected_bbox) in cases {
        let shape =
            Shape::triangle(Vec2::ZERO, Vec2::new(30.0, 0.0), Vec2::new(0.0, 40.0)).radius(radius);
        let record = lower_in(&mut store, (shape, RgbaF32::WHITE));
        let Some(ShapeRecord::Quad(QuadShape::Triangle { geometry, .. })) = record else {
            panic!("a triangle lowers to a triangle quad, got {record:?}");
        };
        assert_eq!(
            geometry.place(Size::ZERO),
            Some(PlacedTriangle {
                centers,
                radius: clamped,
                bbox: expected_bbox,
            }),
            "radius {radius}"
        );
    }
}

#[test]
fn typed_builders_set_the_fields_they_name() {
    let rect = Rect::new(1.0, 2.0, 30.0, 40.0);
    let rect_shape = Shape::rect()
        .at(rect)
        .corners(6.0)
        .coverage(Coverage::Outside);
    assert_eq!(rect_shape.local_rect, Some(rect));
    assert_eq!(rect_shape.corners.to_array(), [6.0; 4]);
    assert_eq!(rect_shape.coverage, Coverage::Outside);
    assert_eq!(
        Shape::rect().coverage,
        Coverage::Inside,
        "inside by default"
    );

    // Every point shape takes its rect and its fit into one placement.
    let placed = PointPlacement {
        local_rect: Some(rect),
        fit: Fit::Contain,
    };
    let mesh = Mesh::new();
    let mesh_shape = Shape::mesh(&mesh).at(rect).fit(Fit::Contain);
    assert!(ptr::eq(mesh_shape.mesh, &raw const mesh));
    assert_eq!(mesh_shape.placement, placed);
    let triangle = Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::Y);
    assert_eq!(
        triangle.placement,
        PointPlacement::NATURAL,
        "natural by default"
    );
    assert_eq!(triangle.at(rect).fit(Fit::Contain).placement, placed);
    let points = [Vec2::ZERO, Vec2::X];
    assert_eq!(
        Shape::polyline(&points)
            .at(rect)
            .fit(Fit::Contain)
            .placement,
        placed
    );

    // A rect of no size only anchors a natural point shape: the corners move by (10, 10).
    let anchored = Shape::triangle(Vec2::ZERO, Vec2::X * 4.0, Vec2::Y * 4.0)
        .at(Rect::new(10.0, 10.0, 0.0, 0.0));
    let Some(ShapeRecord::Quad(QuadShape::Triangle { geometry, .. })) =
        lower((anchored, RgbaF32::WHITE))
    else {
        panic!("a triangle in a rect of no size keeps its record");
    };
    assert_eq!(
        geometry.place(Size::ZERO).map(|placed| placed.centers),
        Some([
            Vec2::new(10.0, 10.0),
            Vec2::new(14.0, 10.0),
            Vec2::new(10.0, 14.0)
        ])
    );

    // A sweep within one full circle stays as authored, its ends included; past one, it clamps
    // to `±2π` with its sign.
    for (sweep, want) in [
        (FRAC_PI_2, FRAC_PI_2),
        (-PI, -PI),
        (TAU, TAU),
        (-TAU, -TAU),
        (3.0 * PI, TAU),
        (-5.0 * PI, -TAU),
        (f32::MAX, TAU),
    ] {
        let arc = Shape::arc(0.25, sweep);
        assert_eq!(arc.start_angle, 0.25, "sweep {sweep}");
        assert_eq!(arc.sweep, want, "sweep {sweep}");
    }
}

#[test]
fn text_noop_rejects_invalid_metrics() {
    use crate::primitives::math::domain::EPS;

    let mut store = RecordStore::default();
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

    // `local_origin` is the one Text scalar `GlyphFont::metrics_valid` doesn't cover; `at_origin` refuses a non-finite one, a finite one doesn't stop a visible run painting.
    for (label, local_origin) in [
        ("no origin", None),
        ("finite origin", Some(Vec2::new(1.0, 2.0))),
    ] {
        let shape = Shape::text(
            store.text.intern("visible"),
            GlyphFont {
                line_height: 19.2,
                ..GlyphFont::new(16.0)
            },
        );
        let shape = match local_origin {
            Some(origin) => shape.at_origin(origin),
            None => shape,
        };
        assert!(
            lower_in(&mut store, (shape, RgbaF32::WHITE)).is_some(),
            "{label}: a visible run does not stop painting over its origin",
        );
    }

    let mut run = |text: &str, color| {
        let shape = Shape::text(store.text.intern(text), GlyphFont::new(16.0));
        lower_in(&mut store, (shape, color))
    };
    assert!(
        run("hidden", RgbaF32::TRANSPARENT).is_some(),
        "a transparent run still sizes its node"
    );
    assert!(
        run("", RgbaF32::WHITE).is_none(),
        "an empty run shapes nothing"
    );

    for (label, font_size, line_height, expected_noop) in cases {
        let shape = Shape::text(
            store.text.intern("visible"),
            GlyphFont {
                line_height,
                slant: FontSlant::Italic,
                ..GlyphFont::new(font_size)
            },
        )
        .wrap(TextWrap::SingleLine)
        .align(Align::TOP_LEFT);
        assert_eq!(
            lower_in(&mut store, (shape.clone(), RgbaF32::WHITE)).is_none(),
            expected_noop,
            "{label}"
        );
        assert_eq!(
            shape.font.slant,
            FontSlant::Italic,
            "{label}: every face axis the builder sets must reach the shape",
        );
    }
}

/// A curve paints nothing when its stroke does or the ramp its color multiplies is transparent everywhere.
#[test]
fn curve_noop_follows_the_stroke_and_the_ramp() {
    let visible = ColorRamp::two_stop(RgbaF32::TRANSPARENT, RgbaF32::WHITE);
    let transparent = ColorRamp::two_stop(RgbaF32::TRANSPARENT, RgbaF32::TRANSPARENT);
    let cases = [
        ("transparent_stroke", RgbaF32::TRANSPARENT, 1.0, None, true),
        ("visible_stroke", RgbaF32::WHITE, 1.0, None, false),
        ("zero_width", RgbaF32::WHITE, 0.0, None, true),
        ("visible_ramp", RgbaF32::WHITE, 1.0, Some(visible), false),
        (
            "transparent_ramp",
            RgbaF32::WHITE,
            1.0,
            Some(transparent),
            true,
        ),
        (
            "transparent_stroke_visible_ramp",
            RgbaF32::TRANSPARENT,
            1.0,
            Some(visible),
            true,
        ),
    ];
    for (label, color, width, ramp, expected_noop) in cases {
        let mut stroke = Stroke::new(color, width);
        if let Some(ramp) = ramp {
            stroke = stroke.with_ramp(ramp);
        }
        assert_eq!(
            lower((Shape::line(Vec2::ZERO, Vec2::X), stroke)).is_none(),
            expected_noop,
            "case: {label}"
        );
    }
}

/// Every curve lowers to a cubic with its tight centerline bbox. A line from (0, 0) to (9, 3) puts
/// its inner points on the thirds, (3, 1) and (6, 2), and is bounded by its ends. The quadratic
/// (0, 0), (6, 6), (12, 0) promotes to inner points `2/3` of the way to its control, (4, 4) and
/// (8, 4), and peaks at `t = ½`, `0.25·0 + 0.5·6 + 0.25·0 = 3`. A cubic passes through as authored;
/// the arch (0, 0), (3, 4), (6, 4), (9, 0) has `y(t) = 12·t(1 − t)`, peaking at 3.
#[test]
fn curves_lower_to_a_cubic_and_its_tight_bbox() {
    let stroke = Stroke::new(RgbaF32::WHITE, 1.0);
    let arch = CubicBezier {
        p0: Vec2::ZERO,
        p1: Vec2::new(3.0, 4.0),
        p2: Vec2::new(6.0, 4.0),
        p3: Vec2::new(9.0, 0.0),
    };
    let cases = [
        (
            "line",
            Shape::line(Vec2::ZERO, Vec2::new(9.0, 3.0)),
            CubicBezier {
                p0: Vec2::ZERO,
                p1: Vec2::new(3.0, 1.0),
                p2: Vec2::new(6.0, 2.0),
                p3: Vec2::new(9.0, 3.0),
            },
            Rect::new(0.0, 0.0, 9.0, 3.0),
        ),
        (
            "quadratic",
            Shape::quadratic_bezier(QuadraticBezier {
                p0: Vec2::ZERO,
                p1: Vec2::new(6.0, 6.0),
                p2: Vec2::new(12.0, 0.0),
            }),
            CubicBezier {
                p0: Vec2::ZERO,
                p1: Vec2::new(4.0, 4.0),
                p2: Vec2::new(8.0, 4.0),
                p3: Vec2::new(12.0, 0.0),
            },
            Rect::new(0.0, 0.0, 12.0, 3.0),
        ),
        (
            "cubic",
            Shape::cubic_bezier(arch),
            arch,
            Rect::new(0.0, 0.0, 9.0, 3.0),
        ),
    ];
    for (label, shape, want_bezier, want_bbox) in cases {
        let Some(ShapeRecord::Curve {
            geometry,
            ramp: CurveRamp::None,
            ..
        }) = lower((shape, stroke))
        else {
            panic!("{label}: a curve without a ramp lowers to a ramp-less curve record");
        };
        assert_eq!(geometry.bezier, want_bezier, "{label}");
        let PlacedStroke::Curve(basis) = geometry.place(Size::ZERO, LineCap::Butt, 1.0) else {
            panic!("{label}: a curve with length places as a curve");
        };
        assert_eq!(basis.bbox(), want_bbox, "{label}");
    }
}

/// An arc and a circle keep their rect, angles and stroke for the owner to resolve: a circle runs
/// the full turn from 0. No sweep under a butt cap paints nothing; under a round cap it keeps its
/// record, whose cap paints as a dot ([`ArcGeometry::place`]).
#[test]
fn arcs_and_circles_lower_to_an_arc_record() {
    let rect = Rect::new(10.0, 20.0, 30.0, 40.0);
    let stroke = Stroke::new(RgbaF32::WHITE, 2.0).with_cap(LineCap::Round);
    let record = |pair| match pair {
        Some(ShapeRecord::Arc {
            cap,
            geometry,
            stroke,
            ..
        }) => (cap, geometry, stroke.width),
        other => panic!("an arc lowers to an arc record, got {other:?}"),
    };
    assert_eq!(
        record(lower((Shape::arc(0.5, PI).at(rect), stroke))),
        (
            LineCap::Round,
            ArcGeometry {
                local_rect: Some(rect),
                start_angle: 0.5,
                sweep: PI,
            },
            2.0,
        ),
    );
    assert_eq!(
        record(lower((Shape::circle(), stroke))),
        (
            LineCap::Round,
            ArcGeometry {
                local_rect: None,
                start_angle: 0.0,
                sweep: TAU,
            },
            2.0,
        ),
    );
    assert!(
        lower((Shape::arc(0.0, 0.0), stroke.with_cap(LineCap::Butt))).is_none(),
        "a butt cap has no extent"
    );
    assert!(lower((Shape::arc(0.0, 0.0), stroke)).is_some());
}

/// A curve of no length keeps its record, and its cap paints alone where it is placed, as SVG
/// paints a zero-length subpath: a round cap a disc and a square cap a square, the stroke width
/// across and centered on the point. A butt cap has no extent, so the curve drops here. Width 4
/// at (3, 4) covers (1, 2) to (5, 6).
#[test]
fn a_curve_of_no_length_paints_its_cap_alone() {
    let wide = Stroke::new(RgbaF32::WHITE, 4.0);
    let at = Vec2::new(3.0, 4.0);
    let dot = Shape::cubic_bezier(CubicBezier {
        p0: at,
        p1: at,
        p2: at,
        p3: at,
    });
    let cases = [
        (
            "round line",
            Shape::line(at, at),
            LineCap::Round,
            Corners::saturating([2.0; 4]),
        ),
        (
            "square line",
            Shape::line(at, at),
            LineCap::Square,
            Corners::ZERO,
        ),
        (
            "round cubic",
            dot,
            LineCap::Round,
            Corners::saturating([2.0; 4]),
        ),
    ];
    for (label, shape, cap, want_corners) in cases {
        let Some(ShapeRecord::Curve { geometry, .. }) = lower((shape, wide.with_cap(cap))) else {
            panic!("{label}: a capped curve of no length keeps its record");
        };
        assert_eq!(
            geometry.place(Size::ZERO, cap, 4.0),
            PlacedStroke::Dot(PlacedBox {
                rect: Rect::new(1.0, 2.0, 4.0, 4.0),
                corners: want_corners,
            }),
            "{label}"
        );
    }
    assert!(
        lower((Shape::line(at, at), wide)).is_none(),
        "a butt cap has no extent"
    );
    assert!(lower((dot, wide)).is_none(), "nor on a cubic");
}

/// Each font axis setter overrides its own field of the run's font and leaves the size, the
/// leading and the other axes as the font named them.
#[test]
fn text_font_axes_override_their_own_field() {
    let mut store = RecordStore::default();
    let font = GlyphFont {
        line_height: 20.0,
        ..GlyphFont::new(14.0)
    };
    let shape = |text| Shape::text(text, font);
    let text = store.text.intern("axes");
    assert_eq!(
        shape(text).family(FontFamily::MONO).font,
        GlyphFont {
            family: FontFamily::MONO,
            ..font
        }
    );
    assert_eq!(
        shape(text).weight(FontWeight::BOLD).font,
        GlyphFont {
            weight: FontWeight::BOLD,
            ..font
        }
    );
    assert_eq!(
        shape(text).slant(FontSlant::Italic).font,
        GlyphFont {
            slant: FontSlant::Italic,
            ..font
        }
    );
}

/// A rect shape's paint bbox takes in its drop shadow's halo, as a node's chrome's does, and stays
/// its rect under an inset one or none: a blur of 4 reaches `4 · 4 = 16` px past its source, moved
/// 2 px down, so over a 100 × 60 owner the bbox runs from (−16, −14) to (116, 78).
#[test]
fn a_rect_shapes_bbox_takes_in_its_drop_shadow() {
    let owner = Size::new(100.0, 60.0);
    let bbox = |shadow: Shadow| {
        let paint = Paint::from(RgbaF32::WHITE).with_shadow(shadow);
        let Some(ShapeRecord::Quad(quad)) = lower((Shape::rect(), paint)) else {
            panic!("a rect lowers to a quad");
        };
        quad.bbox_local(owner)
    };
    let drop = Shadow::drop(RgbaF32::BLACK, Vec2::new(0.0, 2.0), 4.0);
    assert_eq!(bbox(drop), Rect::new(-16.0, -14.0, 132.0, 92.0));
    assert_eq!(bbox(drop.inset()), Rect::new(0.0, 0.0, 100.0, 60.0));
    assert_eq!(bbox(Shadow::NONE), Rect::new(0.0, 0.0, 100.0, 60.0));
}
