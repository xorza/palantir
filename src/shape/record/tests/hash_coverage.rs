//! The exhaustive pin: every named field on every shape either moves the
//! hash or is listed as deliberately excluded.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter};
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::geometry::arc_geometry::ArcGeometry;
use crate::shape::geometry::bezier_geometry::BezierGeometry;
use crate::shape::geometry::box_geometry::{BoxGeometry, BoxOutline};
use crate::shape::geometry::stroke_geometry::{PolylineColorSpan, PolylineGeometry};
use crate::shape::geometry::triangle_geometry::TriangleGeometry;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::record::*;
use crate::shape::record_store::recorded_gradients::GradientId;
use crate::text::family_keys::FamilyKey;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;

/// Every field of every record moves the hash, or is listed as deliberately excluded and must not. The compiler catches only new fields, not one bound and never hashed. Exclusions: `bbox` is derived, spans are frame-local, a polyline's geometry and `Mesh` fold bulk inputs into `content_hash`.
#[test]
fn every_named_field_either_moves_the_hash_or_is_pinned_as_excluded() {
    #[track_caller]
    fn moves(label: &str, a: &ShapeRecord, b: &ShapeRecord) {
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "{label}: the field never reaches the hasher",
        );
    }
    #[track_caller]
    fn excluded(label: &str, a: &ShapeRecord, b: &ShapeRecord) {
        assert_eq!(
            a.content_hash(),
            b.content_hash(),
            "{label}: the field must stay out of the hash",
        );
    }

    let white = RgbaF32::WHITE.to_f16();
    let red = RgbaF32::srgba(1.0, 0.0, 0.0, 1.0).to_f16();
    let solid = LoweredBrush::Solid(white);
    let stroke = LoweredBorder::lower(Border::new(RgbaF32::BLACK, 1.0));
    let stroke2 = LoweredBorder::lower(Border::new(RgbaF32::BLACK, 2.0));
    let rect = |r| Some(Rect::new(r, 0.0, 4.0, 4.0));
    let pt = |x| Vec2::new(x, 0.0);

    let quad_box = |coverage, local_rect, outline, fill, stroke| ShapeRecord::Closed {
        geometry: ClosedGeometry::Box(BoxGeometry {
            local_rect,
            outline,
            coverage,
        }),
        paint: ClosedPaint::Fill(LoweredPaint {
            fill,
            border: stroke,
            shadow: LoweredShadow::lower(Shadow::NONE),
            material: None,
        }),
    };
    let rounded = BoxOutline::Rect(Corners::all(2.0));
    let base = quad_box(Coverage::Inside, rect(0.0), rounded, solid, stroke);
    moves(
        "Quad/Box.coverage",
        &base,
        &quad_box(Coverage::Outside, rect(0.0), rounded, solid, stroke),
    );
    moves(
        "Quad/Box.local_rect",
        &base,
        &quad_box(Coverage::Inside, rect(1.0), rounded, solid, stroke),
    );
    moves(
        "Quad/Box.outline corners",
        &base,
        &quad_box(
            Coverage::Inside,
            rect(0.0),
            BoxOutline::Rect(Corners::all(3.0)),
            solid,
            stroke,
        ),
    );
    // A circle carries no corners, so the outline's kind alone tells it from a sharp rect.
    moves(
        "Quad/Box.outline circle",
        &quad_box(
            Coverage::Inside,
            rect(0.0),
            BoxOutline::Rect(Corners::ZERO),
            solid,
            stroke,
        ),
        &quad_box(
            Coverage::Inside,
            rect(0.0),
            BoxOutline::Circle,
            solid,
            stroke,
        ),
    );
    moves(
        "Quad/Box.fill",
        &base,
        &quad_box(
            Coverage::Inside,
            rect(0.0),
            rounded,
            LoweredBrush::Solid(red),
            stroke,
        ),
    );
    let box_with = |shadow: Shadow, material| ShapeRecord::Closed {
        geometry: ClosedGeometry::Box(BoxGeometry {
            local_rect: rect(0.0),
            outline: rounded,
            coverage: Coverage::Inside,
        }),
        paint: ClosedPaint::Fill(LoweredPaint {
            fill: solid,
            border: stroke,
            shadow: LoweredShadow::lower(shadow),
            material,
        }),
    };
    let sh = Shadow {
        color: RgbaF32::BLACK,
        offset: Vec2::new(1.0, 2.0),
        blur: 3.0,
        spread: 1.0,
        inset: false,
    };
    moves("Quad/Box.shadow", &base, &box_with(sh, None));
    for (label, changed) in [
        (
            "color",
            Shadow {
                color: RgbaF32::WHITE,
                ..sh
            },
        ),
        (
            "offset",
            Shadow {
                offset: Vec2::new(9.0, 2.0),
                ..sh
            },
        ),
        ("blur", Shadow { blur: 4.0, ..sh }),
        ("spread", Shadow { spread: 2.0, ..sh }),
        ("inset", Shadow { inset: true, ..sh }),
    ] {
        moves(
            &format!("Quad/Box.shadow.{label}"),
            &box_with(sh, None),
            &box_with(changed, None),
        );
    }
    moves(
        "Quad/Box.material",
        &base,
        &box_with(Shadow::NONE, Some(LoweredMaterial::fixture(8.0))),
    );
    moves(
        "Quad/Box.border",
        &base,
        &quad_box(Coverage::Inside, rect(0.0), rounded, solid, stroke2),
    );
    // A gradient's content hash rides in its brush; its frame-local id does not move the hash.
    let grad = |id, hash| LoweredBrush::Gradient {
        id: GradientId(id),
        hash: ContentHash::from_u64(hash),
    };
    let grad_box = |fill| quad_box(Coverage::Inside, rect(0.0), rounded, fill, stroke);
    moves(
        "Quad/Box.fill gradient hash",
        &grad_box(grad(0, 1)),
        &grad_box(grad(0, 2)),
    );
    excluded(
        "Quad/Box.fill gradient id",
        &grad_box(grad(0, 1)),
        &grad_box(grad(5, 1)),
    );

    let tri = |corners, radius, placement, fill, stroke| ShapeRecord::Closed {
        geometry: ClosedGeometry::Triangle(TriangleGeometry {
            corners,
            radius,
            placement,
        }),
        paint: ClosedPaint::Fill(LoweredPaint {
            fill,
            border: stroke,
            material: None,
            shadow: LoweredShadow::default(),
        }),
    };
    let corners = [pt(0.0), pt(1.0), pt(2.0)];
    let natural = PointPlacement::NATURAL;
    let base = tri(corners, 1.0, natural, solid, stroke);
    for (label, moved) in [
        ("a", [pt(9.0), pt(1.0), pt(2.0)]),
        ("b", [pt(0.0), pt(9.0), pt(2.0)]),
        ("c", [pt(0.0), pt(1.0), pt(9.0)]),
    ] {
        moves(
            &format!("Quad/Triangle.corners.{label}"),
            &base,
            &tri(moved, 1.0, natural, solid, stroke),
        );
    }
    moves(
        "Quad/Triangle.radius",
        &base,
        &tri(corners, 2.0, natural, solid, stroke),
    );
    for (label, placement) in placements(rect) {
        moves(
            &format!("Quad/Triangle.placement.{label}"),
            &base,
            &tri(corners, 1.0, placement, solid, stroke),
        );
    }
    moves(
        "Quad/Triangle.fill",
        &base,
        &tri(corners, 1.0, natural, LoweredBrush::Solid(red), stroke),
    );
    let grad_tri = |fill| tri(corners, 1.0, natural, fill, stroke);
    moves(
        "Quad/Triangle.fill gradient hash",
        &grad_tri(grad(0, 1)),
        &grad_tri(grad(0, 2)),
    );
    excluded(
        "Quad/Triangle.fill gradient id",
        &grad_tri(grad(0, 1)),
        &grad_tri(grad(5, 1)),
    );
    let ShapeRecord::Closed {
        geometry: ClosedGeometry::Triangle(geometry),
        paint: ClosedPaint::Fill(LoweredPaint { fill, border, .. }),
    } = base
    else {
        unreachable!("`tri` builds a triangle");
    };
    moves(
        "Quad/Triangle.material",
        &base,
        &ShapeRecord::Closed {
            geometry: ClosedGeometry::Triangle(geometry),
            paint: ClosedPaint::Fill(LoweredPaint {
                fill,
                border,
                material: Some(LoweredMaterial::fixture(8.0)),
                shadow: LoweredShadow::default(),
            }),
        },
    );
    moves(
        "Quad/Triangle.border",
        &base,
        &tri(corners, 1.0, natural, solid, stroke2),
    );

    // A stroke's paint, apart from a border's: each of its fields moves the hash of every kind of
    // path, a ramp by its content hash and never by its frame-local id.
    let none = CurveRamp::None;
    let interned = |id, hash| CurveRamp::Interned {
        id: GradientId(id),
        hash: ContentHash::from_u64(hash),
    };
    let line = LoweredStroke::lower(Stroke::new(RgbaF32::BLACK, 1.0), none);
    let line_changes = [
        ("width", LoweredStroke { width: 2.0, ..line }),
        (
            "color",
            LoweredStroke {
                color: RgbaF32::srgba(1.0, 0.0, 0.0, 1.0).to_f16(),
                ..line
            },
        ),
        (
            "cap",
            LoweredStroke {
                cap: LineCap::Round,
                ..line
            },
        ),
        (
            "join",
            LoweredStroke {
                join: LineJoin::Round,
                ..line
            },
        ),
        (
            "ramp present",
            LoweredStroke {
                ramp: interned(0, 1),
                ..line
            },
        ),
    ];

    let polyline = PolylineGeometry {
        points: Span::new(0, 2),
        colors: PolylineColorSpan::PerPoint(Span::new(0, 2)),
        bbox: Rect::ZERO,
        placement: PointPlacement::NATURAL,
        content_hash: ContentHash::from_u64(7),
    };
    let poly = |geometry, stroke| ShapeRecord::Stroke {
        geometry: StrokeGeometry::Polyline(geometry),
        stroke,
    };
    let base = poly(polyline, line);
    moves(
        "Polyline.content_hash",
        &base,
        &poly(
            PolylineGeometry {
                content_hash: ContentHash::from_u64(8),
                ..polyline
            },
            line,
        ),
    );
    for (label, placement) in placements(rect) {
        moves(
            &format!("Polyline.placement.{label}"),
            &base,
            &replaced(base.clone(), placement),
        );
    }
    for (label, stroke) in line_changes {
        moves(
            &format!("Polyline.stroke.{label}"),
            &base,
            &poly(polyline, stroke),
        );
    }
    // The spans are frame-local, the colors' kind is folded into `content_hash` at lowering, and
    // `bbox` derives from the points.
    for (label, geometry) in [
        (
            "points span",
            PolylineGeometry {
                points: Span::new(99, 2),
                ..polyline
            },
        ),
        (
            "colors span",
            PolylineGeometry {
                colors: PolylineColorSpan::PerPoint(Span::new(99, 2)),
                ..polyline
            },
        ),
        (
            "colors kind",
            PolylineGeometry {
                colors: PolylineColorSpan::Single,
                ..polyline
            },
        ),
        (
            "bbox",
            PolylineGeometry {
                bbox: Rect::new(5.0, 5.0, 5.0, 5.0),
                ..polyline
            },
        ),
    ] {
        excluded(&format!("Polyline.{label}"), &base, &poly(geometry, line));
    }

    let recorded = |hash| RecordedText {
        span: Span::new(0, 1),
        hash: ContentHash::from_u64(hash),
    };
    // A struct update varies one metric without restating the rest.
    let face = ShapingFont {
        font_size: 12.0,
        line_height: 14.0,
        family: FamilyKey::from_raw(0),
        weight: FontWeight::REGULAR,
        slant: FontSlant::Normal,
    };
    let text = |local_origin, t, color, font, wrap, align| ShapeRecord::Text {
        local_origin,
        text: t,
        color,
        font,
        wrap,
        align,
    };
    let base = text(
        None,
        recorded(1),
        white,
        face,
        TextWrap::SingleLine,
        Align::CENTER,
    );
    moves(
        "Text.local_origin",
        &base,
        &text(
            Some(pt(1.0)),
            recorded(1),
            white,
            face,
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );
    moves(
        "Text.text",
        &base,
        &text(
            None,
            recorded(2),
            white,
            face,
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );
    moves(
        "Text.color",
        &base,
        &text(
            None,
            recorded(1),
            red,
            face,
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );
    moves(
        "Text.font.font_size",
        &base,
        &text(
            None,
            recorded(1),
            white,
            ShapingFont {
                font_size: 13.0,
                ..face
            },
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );
    moves(
        "Text.font.line_height",
        &base,
        &text(
            None,
            recorded(1),
            white,
            ShapingFont {
                line_height: 15.0,
                ..face
            },
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );
    moves(
        "Text.wrap",
        &base,
        &text(
            None,
            recorded(1),
            white,
            face,
            TextWrap::Wrap,
            Align::CENTER,
        ),
    );
    moves(
        "Text.align",
        &base,
        &text(
            None,
            recorded(1),
            white,
            face,
            TextWrap::SingleLine,
            Align::TOP_LEFT,
        ),
    );
    moves(
        "Text.font.family",
        &base,
        &text(
            None,
            recorded(1),
            white,
            ShapingFont {
                family: FamilyKey::from_raw(1),
                ..face
            },
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );
    moves(
        "Text.font.weight",
        &base,
        &text(
            None,
            recorded(1),
            white,
            ShapingFont {
                weight: FontWeight::BOLD,
                ..face
            },
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );

    let mesh = |local_rect, tint, vertices, idxs, bbox, content_hash| ShapeRecord::Mesh {
        placement: PointPlacement {
            local_rect,
            fit: Fit::Natural,
        },
        tint,
        vertices,
        idxs,
        bbox,
        content_hash: ContentHash::from_u64(content_hash),
    };
    let base = mesh(
        rect(0.0),
        white,
        Span::new(0, 3),
        Span::new(0, 3),
        Rect::ZERO,
        7,
    );
    moves(
        "Mesh.placement.local_rect",
        &base,
        &mesh(
            rect(1.0),
            white,
            Span::new(0, 3),
            Span::new(0, 3),
            Rect::ZERO,
            7,
        ),
    );
    for (label, placement) in placements(rect) {
        moves(
            &format!("Mesh.placement.{label}"),
            &base,
            &replaced(base.clone(), placement),
        );
    }
    moves(
        "Mesh.tint",
        &base,
        &mesh(
            rect(0.0),
            red,
            Span::new(0, 3),
            Span::new(0, 3),
            Rect::ZERO,
            7,
        ),
    );
    moves(
        "Mesh.content_hash",
        &base,
        &mesh(
            rect(0.0),
            white,
            Span::new(0, 3),
            Span::new(0, 3),
            Rect::ZERO,
            8,
        ),
    );
    excluded(
        "Mesh.vertices span",
        &base,
        &mesh(
            rect(0.0),
            white,
            Span::new(99, 3),
            Span::new(0, 3),
            Rect::ZERO,
            7,
        ),
    );
    excluded(
        "Mesh.indices span",
        &base,
        &mesh(
            rect(0.0),
            white,
            Span::new(0, 3),
            Span::new(99, 3),
            Rect::ZERO,
            7,
        ),
    );
    excluded(
        "Mesh.bbox",
        &base,
        &mesh(
            rect(0.0),
            white,
            Span::new(0, 3),
            Span::new(0, 3),
            Rect::new(5.0, 5.0, 5.0, 5.0),
            7,
        ),
    );

    let image =
        |local_rect, tint, source, fit, min_filter, mag_filter, downsample| ShapeRecord::Closed {
            geometry: ClosedGeometry::Box(BoxGeometry {
                local_rect,
                outline: BoxOutline::Rect(Corners::ZERO),
                coverage: Coverage::Inside,
            }),
            paint: ClosedPaint::Image {
                tint,
                source,
                fit,
                min_filter,
                mag_filter,
                downsample,
            },
        };
    let texture = ImageSource::Texture {
        id: TextureId(1),
        size: glam::UVec2::new(2, 3),
        generation: 0,
    };
    let base = image(
        rect(0.0),
        white,
        texture,
        ImageFit::Fill,
        ImageFilter::Linear,
        ImageFilter::Linear,
        ImageDownsample::Single,
    );
    moves(
        "ImagePaint.geometry.local_rect",
        &base,
        &image(
            rect(1.0),
            white,
            texture,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    let ShapeRecord::Closed {
        paint:
            ClosedPaint::Image {
                tint,
                source,
                fit,
                min_filter,
                mag_filter,
                downsample,
                ..
            },
        ..
    } = base
    else {
        unreachable!("`image` builds an image record");
    };
    let shaped = |geometry| ShapeRecord::Closed {
        geometry,
        paint: ClosedPaint::Image {
            tint,
            source,
            fit,
            min_filter,
            mag_filter,
            downsample,
        },
    };
    let triangle = |radius| {
        ClosedGeometry::Triangle(TriangleGeometry {
            corners: [pt(0.0), pt(1.0), pt(2.0)],
            radius,
            placement: PointPlacement::NATURAL,
        })
    };
    moves(
        "ImagePaint.geometry.corners",
        &base,
        &shaped(ClosedGeometry::Box(BoxGeometry {
            local_rect: rect(0.0),
            outline: BoxOutline::Rect(Corners::all(4.0)),
            coverage: Coverage::Inside,
        })),
    );
    moves(
        "ImagePaint.geometry.circle",
        &base,
        &shaped(ClosedGeometry::Box(BoxGeometry {
            local_rect: rect(0.0),
            outline: BoxOutline::Circle,
            coverage: Coverage::Inside,
        })),
    );
    moves(
        "ImagePaint.geometry.coverage",
        &base,
        &shaped(ClosedGeometry::Box(BoxGeometry {
            local_rect: rect(0.0),
            outline: BoxOutline::Rect(Corners::ZERO),
            coverage: Coverage::Outside,
        })),
    );
    moves("ImagePaint.geometry.kind", &base, &shaped(triangle(1.0)));
    moves(
        "ImagePaint.geometry.triangle",
        &shaped(triangle(1.0)),
        &shaped(triangle(2.0)),
    );
    moves(
        "ImagePaint.tint",
        &base,
        &image(
            rect(0.0),
            red,
            texture,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "ImagePaint.source",
        &base,
        &image(
            rect(0.0),
            white,
            ImageSource::GpuView {
                id: TextureId(1),
                epoch: 1,
            },
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "ImagePaint.source.generation",
        &base,
        &image(
            rect(0.0),
            white,
            ImageSource::Texture {
                id: TextureId(1),
                size: glam::UVec2::new(2, 3),
                generation: 1,
            },
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "ImagePaint.fit",
        &base,
        &image(
            rect(0.0),
            white,
            texture,
            ImageFit::Cover,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "ImagePaint.min_filter",
        &base,
        &image(
            rect(0.0),
            white,
            texture,
            ImageFit::Fill,
            ImageFilter::Nearest,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "ImagePaint.mag_filter",
        &base,
        &image(
            rect(0.0),
            white,
            texture,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Nearest,
            ImageDownsample::Single,
        ),
    );
    // Both tap modes share the field's bit range in the hash byte; folding them would let one reuse the other's pixels.
    moves(
        "ImagePaint.downsample=Mean",
        &base,
        &image(
            rect(0.0),
            white,
            texture,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Mean,
        ),
    );
    moves(
        "ImagePaint.downsample=Peak",
        &base,
        &image(
            rect(0.0),
            white,
            texture,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Peak,
        ),
    );
    moves(
        "ImagePaint.downsample Mean vs Peak",
        &image(
            rect(0.0),
            white,
            texture,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Mean,
        ),
        &image(
            rect(0.0),
            white,
            texture,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Peak,
        ),
    );

    let curve = |bezier, stroke| ShapeRecord::Stroke {
        geometry: StrokeGeometry::Bezier(BezierGeometry {
            bezier,
            placement: PointPlacement::NATURAL,
        }),
        stroke,
    };
    let cubic = CubicBezier {
        p0: Vec2::ZERO,
        p1: Vec2::ZERO,
        p2: Vec2::ZERO,
        p3: Vec2::ZERO,
    };
    let base = curve(cubic, line);
    for (label, bezier) in [
        (
            "p0",
            CubicBezier {
                p0: pt(1.0),
                ..cubic
            },
        ),
        (
            "p1",
            CubicBezier {
                p1: pt(1.0),
                ..cubic
            },
        ),
        (
            "p2",
            CubicBezier {
                p2: pt(1.0),
                ..cubic
            },
        ),
        (
            "p3",
            CubicBezier {
                p3: pt(1.0),
                ..cubic
            },
        ),
    ] {
        moves(
            &format!("Curve.bezier.{label}"),
            &base,
            &curve(bezier, line),
        );
    }
    for (label, stroke) in line_changes {
        moves(
            &format!("Curve.stroke.{label}"),
            &base,
            &curve(cubic, stroke),
        );
    }
    let ramped = LoweredStroke {
        ramp: interned(0, 1),
        ..line
    };
    moves(
        "Curve.stroke.ramp hash",
        &curve(cubic, ramped),
        &curve(
            cubic,
            LoweredStroke {
                ramp: interned(0, 2),
                ..line
            },
        ),
    );
    excluded(
        "Curve.stroke.ramp id (frame-local)",
        &curve(cubic, ramped),
        &curve(
            cubic,
            LoweredStroke {
                ramp: interned(7, 1),
                ..line
            },
        ),
    );
    for (label, placement) in placements(rect) {
        moves(
            &format!("Curve.placement.{label}"),
            &base,
            &replaced(base.clone(), placement),
        );
    }

    let arc = |geometry, stroke| ShapeRecord::Stroke {
        geometry: StrokeGeometry::Arc(geometry),
        stroke,
    };
    let quarter = ArcGeometry {
        local_rect: None,
        start_angle: 0.0,
        sweep: 1.0,
    };
    let base = arc(quarter, line);
    for (label, changed) in [
        (
            "local_rect",
            ArcGeometry {
                local_rect: rect(0.0),
                ..quarter
            },
        ),
        (
            "start_angle",
            ArcGeometry {
                start_angle: 0.5,
                ..quarter
            },
        ),
        (
            "sweep",
            ArcGeometry {
                sweep: 2.0,
                ..quarter
            },
        ),
    ] {
        moves(&format!("Arc.arc.{label}"), &base, &arc(changed, line));
    }
    for (label, stroke) in line_changes {
        moves(&format!("Arc.stroke.{label}"), &base, &arc(quarter, stroke));
    }
    // A curve and an arc of one stroke hash apart by their geometry's kind alone.
    moves("Stroke.geometry kind", &curve(cubic, line), &base);
}

/// A placement that differs from [`PointPlacement::NATURAL`] in each field, as `rect` places one.
fn placements(rect: impl Fn(f32) -> Option<Rect>) -> [(&'static str, PointPlacement); 2] {
    [
        (
            "local_rect",
            PointPlacement {
                local_rect: rect(3.0),
                fit: Fit::Natural,
            },
        ),
        (
            "fit",
            PointPlacement {
                local_rect: None,
                fit: Fit::Contain,
            },
        ),
    ]
}

/// `record`, a polyline, a mesh or a curve, with its placement set to `to`.
fn replaced(mut record: ShapeRecord, to: PointPlacement) -> ShapeRecord {
    match &mut record {
        ShapeRecord::Stroke {
            geometry: StrokeGeometry::Polyline(PolylineGeometry { placement, .. }),
            ..
        }
        | ShapeRecord::Mesh { placement, .. }
        | ShapeRecord::Stroke {
            geometry: StrokeGeometry::Bezier(BezierGeometry { placement, .. }),
            ..
        } => *placement = to,
        other => panic!("{other:?} has no point placement"),
    }
    record
}
