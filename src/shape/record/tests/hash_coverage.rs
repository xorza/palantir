//! The exhaustive pin: every named field on every shape either moves the
//! hash or is listed as deliberately excluded.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::geometry::box_geometry::{BoxGeometry, BoxOutline};
use crate::shape::geometry::triangle_geometry::TriangleGeometry;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::record::*;
use crate::shape::record_store::recorded_gradients::GradientId;
use crate::text::family_keys::FamilyKey;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;

/// Every field of every record moves the hash, or is listed as deliberately excluded and must not. The compiler catches only new fields, not one bound and never hashed. Exclusions: `bbox` is derived, spans are frame-local, `Polyline`/`Mesh` fold bulk inputs into `content_hash`.
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

    let quad_box = |coverage, local_rect, outline, fill, stroke| {
        ShapeRecord::Quad(QuadShape::Box {
            geometry: BoxGeometry {
                local_rect,
                outline,
                coverage,
            },
            fill,
            border: stroke,
            shadow: LoweredShadow::lower(Shadow::NONE),
            material: None,
        })
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
    let box_with = |shadow: Shadow, material| {
        ShapeRecord::Quad(QuadShape::Box {
            geometry: BoxGeometry {
                local_rect: rect(0.0),
                outline: rounded,
                coverage: Coverage::Inside,
            },
            fill: solid,
            border: stroke,
            shadow: LoweredShadow::lower(shadow),
            material,
        })
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

    let tri = |corners, radius, placement, fill, stroke| {
        ShapeRecord::Quad(QuadShape::Triangle {
            geometry: TriangleGeometry {
                corners,
                radius,
                placement,
            },
            fill,
            border: stroke,
            material: None,
        })
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
    let ShapeRecord::Quad(QuadShape::Triangle {
        geometry,
        fill,
        border,
        ..
    }) = base
    else {
        unreachable!("`tri` builds a triangle");
    };
    moves(
        "Quad/Triangle.material",
        &base,
        &ShapeRecord::Quad(QuadShape::Triangle {
            geometry,
            fill,
            border,
            material: Some(LoweredMaterial::fixture(8.0)),
        }),
    );
    moves(
        "Quad/Triangle.border",
        &base,
        &tri(corners, 1.0, natural, solid, stroke2),
    );

    let poly =
        |width, color_mode, cap, join, points, colors, bbox, content_hash| ShapeRecord::Polyline {
            width,
            color_mode,
            cap,
            join,
            points,
            colors,
            bbox,
            placement: PointPlacement::NATURAL,
            content_hash: ContentHash::from_u64(content_hash),
        };
    let base = poly(
        1.0,
        ColorMode::Single,
        LineCap::Butt,
        LineJoin::Miter,
        Span::new(0, 2),
        Span::new(0, 1),
        Rect::ZERO,
        7,
    );
    moves(
        "Polyline.content_hash",
        &base,
        &poly(
            1.0,
            ColorMode::Single,
            LineCap::Butt,
            LineJoin::Miter,
            Span::new(0, 2),
            Span::new(0, 1),
            Rect::ZERO,
            8,
        ),
    );
    for (label, placement) in placements(rect) {
        moves(
            &format!("Polyline.placement.{label}"),
            &base,
            &replaced(base.clone(), placement),
        );
    }
    // Folded into `content_hash` at lowering; must not be hashed twice.
    excluded(
        "Polyline.width",
        &base,
        &poly(
            2.0,
            ColorMode::Single,
            LineCap::Butt,
            LineJoin::Miter,
            Span::new(0, 2),
            Span::new(0, 1),
            Rect::ZERO,
            7,
        ),
    );
    excluded(
        "Polyline.color_mode",
        &base,
        &poly(
            1.0,
            ColorMode::PerPoint,
            LineCap::Butt,
            LineJoin::Miter,
            Span::new(0, 2),
            Span::new(0, 1),
            Rect::ZERO,
            7,
        ),
    );
    excluded(
        "Polyline.cap",
        &base,
        &poly(
            1.0,
            ColorMode::Single,
            LineCap::Round,
            LineJoin::Miter,
            Span::new(0, 2),
            Span::new(0, 1),
            Rect::ZERO,
            7,
        ),
    );
    excluded(
        "Polyline.join",
        &base,
        &poly(
            1.0,
            ColorMode::Single,
            LineCap::Butt,
            LineJoin::Round,
            Span::new(0, 2),
            Span::new(0, 1),
            Rect::ZERO,
            7,
        ),
    );
    excluded(
        "Polyline.points span",
        &base,
        &poly(
            1.0,
            ColorMode::Single,
            LineCap::Butt,
            LineJoin::Miter,
            Span::new(99, 2),
            Span::new(0, 1),
            Rect::ZERO,
            7,
        ),
    );
    excluded(
        "Polyline.colors span",
        &base,
        &poly(
            1.0,
            ColorMode::Single,
            LineCap::Butt,
            LineJoin::Miter,
            Span::new(0, 2),
            Span::new(99, 1),
            Rect::ZERO,
            7,
        ),
    );
    excluded(
        "Polyline.bbox",
        &base,
        &poly(
            1.0,
            ColorMode::Single,
            LineCap::Butt,
            LineJoin::Miter,
            Span::new(0, 2),
            Span::new(0, 1),
            Rect::new(5.0, 5.0, 5.0, 5.0),
            7,
        ),
    );

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

    let image = |local_rect, tint, source, fit, min_filter, mag_filter, downsample| {
        ShapeRecord::ImagePaint {
            geometry: ClosedGeometry::Box(BoxGeometry {
                local_rect,
                outline: BoxOutline::Rect(Corners::ZERO),
                coverage: Coverage::Inside,
            }),
            tint,
            source,
            fit,
            min_filter,
            mag_filter,
            downsample,
        }
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
    let ShapeRecord::ImagePaint {
        tint,
        source,
        fit,
        min_filter,
        mag_filter,
        downsample,
        ..
    } = base
    else {
        unreachable!("`image` builds an image record");
    };
    let shaped = |geometry| ShapeRecord::ImagePaint {
        geometry,
        tint,
        source,
        fit,
        min_filter,
        mag_filter,
        downsample,
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

    let curve = |cap, bezier, stroke, ramp| ShapeRecord::Curve {
        cap,
        geometry: BezierGeometry {
            bezier,
            placement: PointPlacement::NATURAL,
        },
        stroke,
        ramp,
    };
    let cubic = CubicBezier {
        p0: Vec2::ZERO,
        p1: Vec2::ZERO,
        p2: Vec2::ZERO,
        p3: Vec2::ZERO,
    };
    let none = CurveRamp::None;
    let interned = |id, hash| CurveRamp::Interned {
        id: GradientId(id),
        hash: ContentHash::from_u64(hash),
    };
    // A native stroke's paint, apart from a border's.
    let line = |color: RgbaF32, width| LoweredStroke::lower(Stroke::new(color, width));
    let stroke = line(RgbaF32::BLACK, 1.0);
    let stroke2 = line(RgbaF32::BLACK, 2.0);
    let red_stroke = line(RgbaF32::srgba(1.0, 0.0, 0.0, 1.0), 1.0);
    let base = curve(LineCap::Butt, cubic, stroke, none);
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
            &curve(LineCap::Butt, bezier, stroke, none),
        );
    }
    moves(
        "Curve.stroke width",
        &base,
        &curve(LineCap::Butt, cubic, stroke2, none),
    );
    moves(
        "Curve.stroke color",
        &base,
        &curve(LineCap::Butt, cubic, red_stroke, none),
    );
    moves(
        "Curve.cap",
        &base,
        &curve(LineCap::Round, cubic, stroke, none),
    );
    // A ramp's content hash moves the hash, its frame-local id does not. Cap and ramp share a byte, so the cap must still move it.
    let ramped = curve(LineCap::Butt, cubic, stroke, interned(0, 1));
    moves("Curve.ramp present", &base, &ramped);
    moves(
        "Curve.ramp hash",
        &ramped,
        &curve(LineCap::Butt, cubic, stroke, interned(0, 2)),
    );
    moves(
        "Curve.cap under a ramp",
        &ramped,
        &curve(LineCap::Round, cubic, stroke, interned(0, 1)),
    );
    excluded(
        "Curve.ramp id (frame-local)",
        &ramped,
        &curve(LineCap::Butt, cubic, stroke, interned(7, 1)),
    );
    for (label, placement) in placements(rect) {
        moves(
            &format!("Curve.placement.{label}"),
            &base,
            &replaced(base.clone(), placement),
        );
    }

    let arc = |cap, geometry, stroke, ramp| ShapeRecord::Arc {
        cap,
        geometry,
        stroke,
        ramp,
    };
    let quarter = ArcGeometry {
        local_rect: None,
        start_angle: 0.0,
        sweep: 1.0,
    };
    let base = arc(LineCap::Butt, quarter, stroke, none);
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
        moves(
            &format!("Arc.arc.{label}"),
            &base,
            &arc(LineCap::Butt, changed, stroke, none),
        );
    }
    moves(
        "Arc.cap",
        &base,
        &arc(LineCap::Round, quarter, stroke, none),
    );
    moves(
        "Arc.stroke",
        &base,
        &arc(LineCap::Butt, quarter, stroke2, none),
    );
    moves(
        "Arc.ramp",
        &base,
        &arc(LineCap::Butt, quarter, stroke, interned(0, 1)),
    );
    excluded(
        "Arc.ramp id (frame-local)",
        &arc(LineCap::Butt, quarter, stroke, interned(0, 1)),
        &arc(LineCap::Butt, quarter, stroke, interned(7, 1)),
    );
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
        ShapeRecord::Polyline { placement, .. }
        | ShapeRecord::Mesh { placement, .. }
        | ShapeRecord::Curve {
            geometry: BezierGeometry { placement, .. },
            ..
        } => *placement = to,
        other => panic!("{other:?} has no point placement"),
    }
    record
}
