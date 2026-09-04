//! The exhaustive pin: every named field on every shape either moves the
//! hash or is listed as deliberately excluded.

use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::primitives::texture_id::TextureId;
use crate::scene::record_store::recorded_gradients::GradientId;
use crate::scene::shapes::hash::compute_record_hash;
use crate::scene::shapes::paint::{LoweredShadow, ShapeStroke};
use crate::scene::shapes::record::*;
use crate::shape::rect::RectKind;
use crate::text::font_family::FontFamily;
use crate::text::font_style::FontStyle;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use glam::Vec2;

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

    let white = RgbaF16::from(RgbaF32::WHITE);
    let red = RgbaF16::from(RgbaF32::srgba(1.0, 0.0, 0.0, 1.0));
    let solid = ShapeBrush::Solid(white);
    let stroke = ShapeStroke::from(Stroke::solid(RgbaF32::BLACK, 1.0));
    let stroke2 = ShapeStroke::from(Stroke::solid(RgbaF32::BLACK, 2.0));
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
        color: RgbaF32::BLACK,
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
        span: Span::new(0, 1),
        hash,
    };
    // The face is one argument now, so a case that varies a metric says
    // so with a struct update instead of restating the other three.
    let face = GlyphFont {
        size_px: 12.0,
        line_height_px: 14.0,
        family: FontFamily::SANS,
        weight: FontWeight::REGULAR,
        style: FontStyle::Normal,
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
                family: FontFamily::MONO,
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
                weight: FontWeight::BOLD,
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
