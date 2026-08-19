use super::*;
use crate::primitives::approx::EPS;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::stroke::Stroke;
use crate::primitives::text_source::TextSource;
use crate::primitives::texture_id::TextureId;
use crate::scene::record_store::recorded_gradients::GradientId;
use crate::scene::shapes::hash::compute_record_hash;
use crate::scene::shapes::paint::{LoweredShadow, ShapeStroke, shadow_paint_rect_local};
use crate::shape::rect::RectKind;
use crate::text::glyph_font::GlyphFont;
use crate::text::{FontFamily, FontWeight};
use glam::Vec2;

#[test]
fn shadow_paint_bbox_tracks_shifted_drop_and_source_bounded_inset() {
    #[derive(Debug)]
    struct DropCase {
        offset: Vec2,
        blur: f32,
        spread: f32,
        expected: Rect,
    }

    let source = Rect::new(10.0, 20.0, 30.0, 40.0);
    let cases = [
        DropCase {
            offset: Vec2::new(12.0, 7.0),
            blur: 4.0,
            spread: 2.0,
            expected: Rect::new(8.0, 13.0, 58.0, 68.0),
        },
        DropCase {
            offset: Vec2::new(-9.0, -11.0),
            blur: 3.0,
            spread: 5.0,
            expected: Rect::new(-13.0, -5.0, 58.0, 68.0),
        },
        DropCase {
            offset: Vec2::new(4.0, -3.0),
            blur: 2.0,
            spread: -5.0,
            expected: Rect::new(8.0, 11.0, 42.0, 52.0),
        },
    ];

    for case in cases {
        assert_eq!(
            shadow_paint_rect_local(
                Some(source),
                Size::ZERO,
                case.offset,
                case.blur,
                case.spread,
                false,
            ),
            case.expected,
            "{case:?}",
        );
    }

    assert_eq!(
        shadow_paint_rect_local(
            Some(source),
            Size::ZERO,
            Vec2::new(100.0, -100.0),
            20.0,
            8.0,
            true,
        ),
        source,
        "inset paint remains clipped to its source rect",
    );
}

/// **The hash-schedule sweep.** For every field of every record,
/// change it and assert the hash moves — or, for the fields
/// deliberately left out, assert it does *not*.
///
/// This is the half the compiler cannot do. Naming every field in
/// `compute_record_hash` (no `..`) makes a *new* field a build
/// error, but nothing stops an existing one being bound and then
/// never written to the hasher. A field missing from the hash means
/// two records that differ share one, which damage diff reads as
/// "unchanged" — the repaint is skipped and the stale pixels stay.
///
/// The exclusions are asserted just as hard, because each is
/// load-bearing rather than an oversight: `bbox` is derived from
/// geometry already hashed, spans are frame-local arena offsets that
/// must *not* perturb a cross-frame comparison, and
/// `Polyline`/`Mesh` fold their bulk inputs into `content_hash` at
/// lowering.
#[test]
fn every_named_field_either_moves_the_hash_or_is_pinned_as_excluded() {
    #[track_caller]
    fn moves(label: &str, a: &ShapeRecord, b: &ShapeRecord) {
        assert_ne!(
            compute_record_hash(a),
            compute_record_hash(b),
            "{label}: the field never reaches the hasher",
        );
    }
    #[track_caller]
    fn excluded(label: &str, a: &ShapeRecord, b: &ShapeRecord) {
        assert_eq!(
            compute_record_hash(a),
            compute_record_hash(b),
            "{label}: the field must stay out of the hash",
        );
    }

    let white = ColorF16::from(Color::WHITE);
    let red = ColorF16::from(Color::rgba(1.0, 0.0, 0.0, 1.0));
    let solid = ShapeBrush::Solid(white);
    let stroke = ShapeStroke::from(Stroke::solid(Color::BLACK, 1.0));
    let stroke2 = ShapeStroke::from(Stroke::solid(Color::BLACK, 2.0));
    let rect = |r| Some(Rect::new(r, 0.0, 4.0, 4.0));
    let pt = |x| Vec2::new(x, 0.0);

    // --- Quad / Rect -------------------------------------------
    let quad_rect = |kind, local_rect, corners, fill, stroke, fill_grad_hash| {
        ShapeRecord::Quad(QuadShape::Rect {
            kind,
            local_rect,
            corners,
            fill,
            stroke,
            fill_grad_hash,
        })
    };
    let base = quad_rect(
        RectKind::Rounded,
        rect(0.0),
        Corners::all(2.0),
        solid,
        stroke,
        0,
    );
    moves(
        "Quad/Rect.kind",
        &base,
        &quad_rect(
            RectKind::Windowed,
            rect(0.0),
            Corners::all(2.0),
            solid,
            stroke,
            0,
        ),
    );
    moves(
        "Quad/Rect.local_rect",
        &base,
        &quad_rect(
            RectKind::Rounded,
            rect(1.0),
            Corners::all(2.0),
            solid,
            stroke,
            0,
        ),
    );
    moves(
        "Quad/Rect.corners",
        &base,
        &quad_rect(
            RectKind::Rounded,
            rect(0.0),
            Corners::all(3.0),
            solid,
            stroke,
            0,
        ),
    );
    moves(
        "Quad/Rect.fill",
        &base,
        &quad_rect(
            RectKind::Rounded,
            rect(0.0),
            Corners::all(2.0),
            ShapeBrush::Solid(red),
            stroke,
            0,
        ),
    );
    moves(
        "Quad/Rect.stroke",
        &base,
        &quad_rect(
            RectKind::Rounded,
            rect(0.0),
            Corners::all(2.0),
            solid,
            stroke2,
            0,
        ),
    );
    // `fill_grad_hash` stands in for the gradient's content, so it
    // only participates when the fill *is* a gradient — with a solid
    // it is deliberately unread.
    excluded(
        "Quad/Rect.fill_grad_hash under a solid fill",
        &base,
        &quad_rect(
            RectKind::Rounded,
            rect(0.0),
            Corners::all(2.0),
            solid,
            stroke,
            9,
        ),
    );
    let grad = ShapeBrush::Gradient(GradientId(0));
    moves(
        "Quad/Rect.fill_grad_hash under a gradient fill",
        &quad_rect(
            RectKind::Rounded,
            rect(0.0),
            Corners::all(2.0),
            grad,
            stroke,
            1,
        ),
        &quad_rect(
            RectKind::Rounded,
            rect(0.0),
            Corners::all(2.0),
            grad,
            stroke,
            2,
        ),
    );

    // --- Quad / Shadow -----------------------------------------
    let shadow = |local_rect, corners, sh| {
        ShapeRecord::Quad(QuadShape::Shadow {
            local_rect,
            corners,
            shadow: LoweredShadow::from(sh),
        })
    };
    let sh = Shadow {
        color: Color::BLACK,
        offset: Vec2::new(1.0, 2.0),
        blur: 3.0,
        spread: 1.0,
        inset: false,
    };
    let base = shadow(rect(0.0), Corners::all(2.0), sh);
    moves(
        "Quad/Shadow.local_rect",
        &base,
        &shadow(rect(1.0), Corners::all(2.0), sh),
    );
    moves(
        "Quad/Shadow.corners",
        &base,
        &shadow(rect(0.0), Corners::all(3.0), sh),
    );
    for (label, changed) in [
        (
            "color",
            Shadow {
                color: Color::WHITE,
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
            &format!("Quad/Shadow.shadow.{label}"),
            &base,
            &shadow(rect(0.0), Corners::all(2.0), changed),
        );
    }

    // --- Quad / Triangle ---------------------------------------
    let tri = |a, b, c, radius, fill, stroke, bbox| {
        ShapeRecord::Quad(QuadShape::Triangle {
            a,
            b,
            c,
            radius,
            fill,
            stroke,
            bbox,
        })
    };
    let base = tri(pt(0.0), pt(1.0), pt(2.0), 1.0, white, stroke, Rect::ZERO);
    moves(
        "Quad/Triangle.a",
        &base,
        &tri(pt(9.0), pt(1.0), pt(2.0), 1.0, white, stroke, Rect::ZERO),
    );
    moves(
        "Quad/Triangle.b",
        &base,
        &tri(pt(0.0), pt(9.0), pt(2.0), 1.0, white, stroke, Rect::ZERO),
    );
    moves(
        "Quad/Triangle.c",
        &base,
        &tri(pt(0.0), pt(1.0), pt(9.0), 1.0, white, stroke, Rect::ZERO),
    );
    moves(
        "Quad/Triangle.radius",
        &base,
        &tri(pt(0.0), pt(1.0), pt(2.0), 2.0, white, stroke, Rect::ZERO),
    );
    moves(
        "Quad/Triangle.fill",
        &base,
        &tri(pt(0.0), pt(1.0), pt(2.0), 1.0, red, stroke, Rect::ZERO),
    );
    moves(
        "Quad/Triangle.stroke",
        &base,
        &tri(pt(0.0), pt(1.0), pt(2.0), 1.0, white, stroke2, Rect::ZERO),
    );
    excluded(
        "Quad/Triangle.bbox (derived from a/b/c/radius)",
        &base,
        &tri(
            pt(0.0),
            pt(1.0),
            pt(2.0),
            1.0,
            white,
            stroke,
            Rect::new(5.0, 5.0, 5.0, 5.0),
        ),
    );

    // --- Polyline ----------------------------------------------
    let poly =
        |width, color_mode, cap, join, points, colors, bbox, content_hash| ShapeRecord::Polyline {
            width,
            color_mode,
            cap,
            join,
            points,
            colors,
            bbox,
            content_hash,
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
    // Everything else is folded into `content_hash` at lowering, so
    // the record copies must not be hashed a second time.
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

    // --- Text ---------------------------------------------------
    let recorded = |hash| RecordedText {
        source: TextSource {
            span: Span::new(0, 1),
        },
        hash,
    };
    // The face is one argument now, so a case that varies a metric says
    // so with a struct update instead of restating the other three.
    let face = GlyphFont {
        size_px: 12.0,
        line_height_px: 14.0,
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
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
        "Text.font.size_px",
        &base,
        &text(
            None,
            recorded(1),
            white,
            GlyphFont {
                size_px: 13.0,
                ..face
            },
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );
    moves(
        "Text.font.line_height_px",
        &base,
        &text(
            None,
            recorded(1),
            white,
            GlyphFont {
                line_height_px: 15.0,
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
            GlyphFont {
                family: FontFamily::Mono,
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
            GlyphFont {
                weight: FontWeight::Bold,
                ..face
            },
            TextWrap::SingleLine,
            Align::CENTER,
        ),
    );

    // --- Mesh ---------------------------------------------------
    let mesh = |local_rect, tint, vertices, indices, bbox, content_hash| ShapeRecord::Mesh {
        local_rect,
        tint,
        vertices,
        indices,
        bbox,
        content_hash,
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
        "Mesh.local_rect",
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

    // --- Image --------------------------------------------------
    let image =
        |local_rect, tint, source, fit, min_filter, mag_filter, downsample| ShapeRecord::Image {
            local_rect,
            tint,
            source,
            fit,
            min_filter,
            mag_filter,
            downsample,
        };
    let tex = ImageSource::Texture {
        id: TextureId(1),
        size: glam::UVec2::new(2, 3),
    };
    let base = image(
        rect(0.0),
        white,
        tex,
        ImageFit::Fill,
        ImageFilter::Linear,
        ImageFilter::Linear,
        ImageDownsample::Single,
    );
    moves(
        "Image.local_rect",
        &base,
        &image(
            rect(1.0),
            white,
            tex,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "Image.tint",
        &base,
        &image(
            rect(0.0),
            red,
            tex,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "Image.source",
        &base,
        &image(
            rect(0.0),
            white,
            ImageSource::GpuView { epoch: 1 },
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "Image.fit",
        &base,
        &image(
            rect(0.0),
            white,
            tex,
            ImageFit::Cover,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "Image.min_filter",
        &base,
        &image(
            rect(0.0),
            white,
            tex,
            ImageFit::Fill,
            ImageFilter::Nearest,
            ImageFilter::Linear,
            ImageDownsample::Single,
        ),
    );
    moves(
        "Image.mag_filter",
        &base,
        &image(
            rect(0.0),
            white,
            tex,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Nearest,
            ImageDownsample::Single,
        ),
    );
    // Both tap modes, because they share the field's bit range in the hash
    // byte: folding them onto one value would let a mean-sampled image reuse a
    // peak-sampled one's painted pixels.
    moves(
        "Image.downsample=Mean",
        &base,
        &image(
            rect(0.0),
            white,
            tex,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Mean,
        ),
    );
    moves(
        "Image.downsample=Peak",
        &base,
        &image(
            rect(0.0),
            white,
            tex,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Peak,
        ),
    );
    moves(
        "Image.downsample Mean vs Peak",
        &image(
            rect(0.0),
            white,
            tex,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Mean,
        ),
        &image(
            rect(0.0),
            white,
            tex,
            ImageFit::Fill,
            ImageFilter::Linear,
            ImageFilter::Linear,
            ImageDownsample::Peak,
        ),
    );

    // --- Curve --------------------------------------------------
    let curve = |basis, width, fill, fill_grad_hash, cap, bbox| ShapeRecord::Curve {
        basis,
        width,
        fill,
        fill_grad_hash,
        cap,
        bbox,
    };
    let cubic = CurveBasis::Cubic {
        p0: Vec2::ZERO,
        p1: Vec2::ZERO,
        p2: Vec2::ZERO,
        p3: Vec2::ZERO,
    };
    let base = curve(cubic, 1.0, solid, 0, LineCap::Butt, Rect::ZERO);
    moves(
        "Curve.basis",
        &base,
        &curve(
            CurveBasis::Arc {
                center: Vec2::ZERO,
                radius: 1.0,
                a0: 0.0,
                a1: 1.0,
            },
            1.0,
            solid,
            0,
            LineCap::Butt,
            Rect::ZERO,
        ),
    );
    moves(
        "Curve.width",
        &base,
        &curve(cubic, 2.0, solid, 0, LineCap::Butt, Rect::ZERO),
    );
    moves(
        "Curve.fill",
        &base,
        &curve(
            cubic,
            1.0,
            ShapeBrush::Solid(red),
            0,
            LineCap::Butt,
            Rect::ZERO,
        ),
    );
    moves(
        "Curve.cap",
        &base,
        &curve(cubic, 1.0, solid, 0, LineCap::Round, Rect::ZERO),
    );
    moves(
        "Curve.fill_grad_hash under a gradient fill",
        &curve(cubic, 1.0, grad, 1, LineCap::Butt, Rect::ZERO),
        &curve(cubic, 1.0, grad, 2, LineCap::Butt, Rect::ZERO),
    );
    excluded(
        "Curve.bbox (derived from basis/width/cap)",
        &base,
        &curve(
            cubic,
            1.0,
            solid,
            0,
            LineCap::Butt,
            Rect::new(5.0, 5.0, 5.0, 5.0),
        ),
    );
}

/// A mesh whose vertex hull overflows its owner box (a rotated / scaled
/// glyph) must report that hull as its paint bbox. Returning the owner
/// rect instead makes partial damage too small — the overflow paints with
/// cut vertices and leaves leftover pixels when it changes. Regression for
/// the subscription-glyph triangle.
#[test]
fn mesh_paint_bbox_is_vertex_hull_not_owner_rect() {
    let owner = Size::new(13.0, 13.0);
    // Hull reaches left/up past the owner origin and right/down past its
    // size — i.e. paints outside the owner box on every side.
    let hull = Rect {
        min: Vec2::new(-5.0, -4.0),
        size: Size::new(25.0, 24.0),
    };
    let mesh = |local_rect| ShapeRecord::Mesh {
        local_rect,
        tint: ColorF16::from(Color::WHITE),
        vertices: Span::new(0, 3),
        indices: Span::new(0, 3),
        bbox: hull,
        content_hash: 0,
    };

    assert_eq!(
        mesh(None).bbox_local(owner),
        hull,
        "the paint bbox is the vertex hull, not the owner rect"
    );

    // `local_rect` translates the hull (its size still comes from the
    // vertices, not `local_rect.size`).
    let offset = Rect {
        min: Vec2::new(2.0, 3.0),
        size: Size::new(99.0, 99.0),
    };
    assert_eq!(
        mesh(Some(offset)).bbox_local(owner),
        Rect {
            min: hull.min + offset.min,
            size: hull.size,
        },
        "local_rect offsets the hull; the size is unchanged"
    );
}

/// Same rectangle payload, different paint kind: switching to a
/// windowed rect inverts the painted region, so a hash collision
/// would make damage diff skip the repaint.
///
/// The same risk, one level up: all three quad shapes share
/// [`ShapeRecord::Quad`]'s discriminant, so `QuadShape`'s own is the
/// only thing separating a rectangle, a shadow, and a triangle over
/// the same box — and each shape's own fields have to reach the
/// hasher through the merged arm. Every case here is a repaint that
/// damage diff would skip on a collision.
#[test]
fn quad_shapes_hash_apart() {
    let fill = ShapeBrush::Solid(ColorF16::from(Color::WHITE));
    let stroke = ShapeStroke::from(Stroke::solid(Color::BLACK, 2.0));
    let corners = Corners::all(8.0);
    let rect = |kind| {
        ShapeRecord::Quad(QuadShape::Rect {
            kind,
            local_rect: None,
            corners,
            fill,
            stroke,
            fill_grad_hash: 0,
        })
    };

    // Same rectangle payload, different paint kind.
    assert_ne!(
        compute_record_hash(&rect(RectKind::Rounded)),
        compute_record_hash(&rect(RectKind::Windowed)),
    );

    // Same rounded box, three different shapes.
    let shadow = ShapeRecord::Quad(QuadShape::Shadow {
        local_rect: None,
        corners,
        shadow: LoweredShadow::from(Shadow::default()),
    });
    let triangle = ShapeRecord::Quad(QuadShape::Triangle {
        a: Vec2::ZERO,
        b: Vec2::ZERO,
        c: Vec2::ZERO,
        radius: 0.0,
        fill: ColorF16::from(Color::WHITE),
        stroke,
        bbox: Rect::ZERO,
    });
    let mut seen = Vec::new();
    for (label, record) in [
        ("rounded", rect(RectKind::Rounded)),
        ("windowed", rect(RectKind::Windowed)),
        ("shadow", shadow),
        ("triangle", triangle),
    ] {
        let hash = compute_record_hash(&record);
        assert!(
            !seen.contains(&hash),
            "quad shape `{label}` collided with an earlier shape's hash",
        );
        seen.push(hash);
    }
}

/// Cubics and arcs share [`ShapeRecord::Curve`]'s discriminant, so
/// [`CurveBasis`]'s own is the only thing separating their hashes —
/// and the arc's own fields have to reach the hasher through the
/// merged arm. A collision either way would make damage diff skip a
/// repaint when a stroke changes shape.
#[test]
fn curve_and_arc_bases_hash_apart() {
    let fill = ShapeBrush::Solid(ColorF16::from(Color::WHITE));
    let curve = |basis| ShapeRecord::Curve {
        basis,
        width: 2.0,
        fill,
        fill_grad_hash: 0,
        cap: LineCap::Butt,
        bbox: Rect::ZERO,
    };
    let arc = |center, radius, a0, a1| {
        curve(CurveBasis::Arc {
            center,
            radius,
            a0,
            a1,
        })
    };
    let baseline = arc(Vec2::ZERO, 4.0, 0.0, 1.0);

    // Every field the two bases don't share is identical here, so
    // only `CurveBasis`'s discriminant can tell these two apart.
    assert_ne!(
        compute_record_hash(&baseline),
        compute_record_hash(&curve(CurveBasis::Cubic {
            p0: Vec2::ZERO,
            p1: Vec2::ZERO,
            p2: Vec2::ZERO,
            p3: Vec2::ZERO,
        })),
        "a degenerate cubic must not collide with an arc",
    );

    for (label, other) in [
        ("center", arc(Vec2::new(1.0, 0.0), 4.0, 0.0, 1.0)),
        ("radius", arc(Vec2::ZERO, 5.0, 0.0, 1.0)),
        ("a0", arc(Vec2::ZERO, 4.0, 0.5, 1.0)),
        ("a1", arc(Vec2::ZERO, 4.0, 0.0, 1.5)),
    ] {
        assert_ne!(
            compute_record_hash(&baseline),
            compute_record_hash(&other),
            "arc `{label}` escaped the hash schedule",
        );
    }
}

#[test]
fn shape_mesh_hash_excludes_span_offsets() {
    let tint = ColorF16::from(Color {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    });
    let a = ShapeRecord::Mesh {
        local_rect: None,
        tint,
        vertices: Span::new(0, 3),
        indices: Span::new(0, 3),
        bbox: Rect::ZERO,
        content_hash: 0xdead_beef,
    };
    let b = ShapeRecord::Mesh {
        local_rect: None,
        tint,
        vertices: Span::new(1234, 3),
        indices: Span::new(5678, 3),
        bbox: Rect::ZERO,
        content_hash: 0xdead_beef,
    };
    assert_eq!(compute_record_hash(&a), compute_record_hash(&b));

    let with_rect = |rect| ShapeRecord::Mesh {
        local_rect: Some(rect),
        tint,
        vertices: Span::new(0, 3),
        indices: Span::new(0, 3),
        bbox: Rect::ZERO,
        content_hash: 0xdead_beef,
    };
    let zero = compute_record_hash(&with_rect(Rect::ZERO));
    assert_eq!(
        zero,
        compute_record_hash(&with_rect(Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS,))),
    );
    assert_ne!(
        zero,
        compute_record_hash(&with_rect(Rect::new(EPS * 2.0, 0.0, 0.0, 0.0))),
    );
}

/// A view composite and a texture draw share `Image`'s record
/// discriminant, so only [`ImageSource`]'s own separates their hashes
/// — and the view's `epoch` has to reach the hasher through the merged
/// arm. A collision either way makes damage diff skip a repaint: a view
/// that bumped its epoch would keep its stale texture on screen.
#[test]
fn image_source_hashes_apart_by_source() {
    let image = |source| ShapeRecord::Image {
        local_rect: None,
        tint: ColorF16::from(Color::WHITE),
        source,
        fit: ImageFit::Fill,
        min_filter: ImageFilter::Linear,
        mag_filter: ImageFilter::Linear,
        downsample: ImageDownsample::Single,
    };
    // Both sources carry one u64-shaped payload of the same value,
    // so the source tag is the only thing telling these two apart.
    let view = compute_record_hash(&image(ImageSource::GpuView { epoch: 7 }));
    assert_ne!(
        view,
        compute_record_hash(&image(ImageSource::Texture {
            id: TextureId(7),
            size: glam::UVec2::ZERO,
        })),
        "a texture id must not collide with an epoch of the same value",
    );
    assert_ne!(
        view,
        compute_record_hash(&image(ImageSource::GpuView { epoch: 8 })),
        "a bumped epoch must move the hash, or the view never repaints",
    );
    assert_eq!(
        view,
        compute_record_hash(&image(ImageSource::GpuView { epoch: 7 })),
        "a held epoch must hold the hash, or a static view never culls",
    );
}

#[test]
fn shape_image_hash_distinguishes_handle_dimensions_tint_and_filters() {
    let make = |id: TextureId,
                size: glam::UVec2,
                tint: Color,
                min_filter: ImageFilter,
                mag_filter: ImageFilter| {
        ShapeRecord::Image {
            local_rect: None,
            tint: ColorF16::from(tint),
            source: ImageSource::Texture { id, size },
            fit: ImageFit::Fill,
            min_filter,
            mag_filter,
            downsample: ImageDownsample::Single,
        }
    };
    let size = glam::UVec2::new(64, 64);
    let baseline = compute_record_hash(&make(
        TextureId(0xa),
        size,
        Color::WHITE,
        ImageFilter::Linear,
        ImageFilter::Linear,
    ));
    assert_ne!(
        baseline,
        compute_record_hash(&make(
            TextureId(0xb),
            size,
            Color::WHITE,
            ImageFilter::Linear,
            ImageFilter::Linear,
        ))
    );
    for changed_size in [
        glam::UVec2::new(size.x + (1 << 16), size.y),
        glam::UVec2::new(size.x, size.y + (1 << 16)),
    ] {
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xa),
                changed_size,
                Color::WHITE,
                ImageFilter::Linear,
                ImageFilter::Linear,
            ))
        );
    }
    assert_ne!(
        baseline,
        compute_record_hash(&make(
            TextureId(0xa),
            size,
            Color::rgba(1.0, 0.0, 0.0, 1.0),
            ImageFilter::Linear,
            ImageFilter::Linear,
        ))
    );
    assert_ne!(
        baseline,
        compute_record_hash(&make(
            TextureId(0xa),
            size,
            Color::WHITE,
            ImageFilter::Nearest,
            ImageFilter::Linear,
        ))
    );
    assert_ne!(
        baseline,
        compute_record_hash(&make(
            TextureId(0xa),
            size,
            Color::WHITE,
            ImageFilter::Linear,
            ImageFilter::Nearest,
        ))
    );
}
