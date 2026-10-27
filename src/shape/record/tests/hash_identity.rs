//! Shapes that must hash apart, and the spans that must not count.

use crate::common::hash;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::math::domain::EPS;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::record::*;
use crate::shape::rect::RectKind;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;

/// Same rectangle payload, different paint kind (a windowed rect inverts the painted region), and all three quad shapes share [`ShapeRecord::Quad`]'s discriminant, so `QuadShape`'s own must separate them and each shape's fields must reach the hasher. A collision makes damage diff skip a repaint.
#[test]
fn quad_shapes_hash_apart() {
    let fill = ShapeBrush::Solid(RgbaF32::WHITE.to_f16());
    let stroke = ShapeStroke::from(Stroke::new(RgbaF32::BLACK, 2.0));
    let corners = Corners::all(8.0);
    let rect = |kind| {
        ShapeRecord::Quad(QuadShape::Rect {
            kind,
            local_rect: None,
            corners,
            fill,
            border: stroke,
        })
    };

    // Same rectangle payload, different paint kind.
    assert_ne!(
        rect(RectKind::Rounded).content_hash(),
        rect(RectKind::Windowed).content_hash(),
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
        fill: RgbaF32::WHITE.to_f16(),
        border: stroke,
        bbox: Rect::ZERO,
    });
    let mut seen = Vec::new();
    for (label, record) in [
        ("rounded", rect(RectKind::Rounded)),
        ("windowed", rect(RectKind::Windowed)),
        ("shadow", shadow),
        ("triangle", triangle),
    ] {
        let hash = record.content_hash();
        assert!(
            !seen.contains(&hash),
            "quad shape `{label}` collided with an earlier shape's hash",
        );
        seen.push(hash);
    }
}

/// Cubics and arcs share [`ShapeRecord::Curve`]'s discriminant, so [`CurveBasis`]'s own must separate them and the arc's fields must reach the hasher.
#[test]
fn curve_and_arc_bases_hash_apart() {
    let stroke = ShapeStroke::from(Stroke::new(RgbaF32::WHITE, 2.0));
    let curve = |basis| ShapeRecord::Curve {
        cap: LineCap::Butt,
        basis,
        stroke,
        bbox: Rect::ZERO,
        ramp: CurveRamp::None,
    };
    let arc = |center, radius, start_angle, sweep| {
        curve(CurveBasis::Arc(CircularArc {
            center,
            radius,
            start_angle,
            sweep,
        }))
    };
    let baseline = arc(Vec2::ZERO, 4.0, 0.0, 1.0);

    // Every non-shared field is identical, so only `CurveBasis`'s discriminant differs.
    assert_ne!(
        baseline.content_hash(),
        curve(CurveBasis::Cubic(CubicBezier {
            p0: Vec2::ZERO,
            p1: Vec2::ZERO,
            p2: Vec2::ZERO,
            p3: Vec2::ZERO,
        }))
        .content_hash(),
        "a degenerate cubic must not collide with an arc",
    );

    for (label, other) in [
        ("center", arc(Vec2::new(1.0, 0.0), 4.0, 0.0, 1.0)),
        ("radius", arc(Vec2::ZERO, 5.0, 0.0, 1.0)),
        ("start_angle", arc(Vec2::ZERO, 4.0, 0.5, 1.0)),
        ("sweep", arc(Vec2::ZERO, 4.0, 0.0, 1.5)),
    ] {
        assert_ne!(
            baseline.content_hash(),
            other.content_hash(),
            "arc `{label}` escaped the hash schedule",
        );
    }
}

#[test]
fn shape_mesh_hash_excludes_span_offsets() {
    let tint = RgbaF32 {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    }
    .to_f16();
    let a = ShapeRecord::Mesh {
        local_rect: None,
        tint,
        vertices: Span::new(0, 3),
        idxs: Span::new(0, 3),
        bbox: Rect::ZERO,
        content_hash: 0xdead_beef,
    };
    let b = ShapeRecord::Mesh {
        local_rect: None,
        tint,
        vertices: Span::new(1234, 3),
        idxs: Span::new(5678, 3),
        bbox: Rect::ZERO,
        content_hash: 0xdead_beef,
    };
    assert_eq!(a.content_hash(), b.content_hash());

    let with_rect = |rect| ShapeRecord::Mesh {
        local_rect: Some(rect),
        tint,
        vertices: Span::new(0, 3),
        idxs: Span::new(0, 3),
        bbox: Rect::ZERO,
        content_hash: 0xdead_beef,
    };
    let zero = with_rect(Rect::ZERO).content_hash();
    assert_eq!(
        zero,
        with_rect(Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS,)).content_hash(),
    );
    assert_ne!(
        zero,
        with_rect(Rect::new(EPS * 2.0, 0.0, 0.0, 0.0)).content_hash(),
    );
}

/// A view composite and a texture draw share `Image`'s discriminant, so [`ImageSource`]'s must separate them and the view's `epoch` must reach the hasher, or a bumped epoch keeps a stale texture.
#[test]
fn image_source_hashes_apart_by_source() {
    let image = |source| ShapeRecord::Image {
        local_rect: None,
        tint: RgbaF32::WHITE.to_f16(),
        source,
        fit: ImageFit::Fill,
        min_filter: ImageFilter::Linear,
        mag_filter: ImageFilter::Linear,
        downsample: ImageDownsample::Single,
    };
    // Same u64-shaped payload, so only the source tag differs.
    let view = image(ImageSource::GpuView { epoch: 7 }).content_hash();
    assert_ne!(
        view,
        image(ImageSource::Texture {
            id: TextureId(7),
            size: glam::UVec2::ZERO,
            generation: 0,
        })
        .content_hash(),
        "a texture id must not collide with an epoch of the same value",
    );
    let texture = |generation| {
        image(ImageSource::Texture {
            id: TextureId(7),
            size: glam::UVec2::ZERO,
            generation,
        })
        .content_hash()
    };
    assert_ne!(
        texture(0),
        texture(1),
        "a write must move the hash, or a rewritten texture never repaints",
    );
    assert_ne!(
        view,
        image(ImageSource::GpuView { epoch: 8 }).content_hash(),
        "a bumped epoch must move the hash, or the view never repaints",
    );
    assert_eq!(
        view,
        image(ImageSource::GpuView { epoch: 7 }).content_hash(),
        "a held epoch must hold the hash, or a static view never culls",
    );
}

#[test]
fn shape_image_hash_distinguishes_handle_dimensions_tint_and_filters() {
    let make = |id: TextureId,
                size: glam::UVec2,
                tint: RgbaF32,
                min_filter: ImageFilter,
                mag_filter: ImageFilter| {
        ShapeRecord::Image {
            local_rect: None,
            tint: tint.to_f16(),
            source: ImageSource::Texture {
                id,
                size,
                generation: 0,
            },
            fit: ImageFit::Fill,
            min_filter,
            mag_filter,
            downsample: ImageDownsample::Single,
        }
    };
    let size = glam::UVec2::new(64, 64);
    let baseline = make(
        TextureId(0xa),
        size,
        RgbaF32::WHITE,
        ImageFilter::Linear,
        ImageFilter::Linear,
    )
    .content_hash();
    assert_ne!(
        baseline,
        make(
            TextureId(0xb),
            size,
            RgbaF32::WHITE,
            ImageFilter::Linear,
            ImageFilter::Linear,
        )
        .content_hash()
    );
    for changed_size in [
        glam::UVec2::new(size.x + (1 << 16), size.y),
        glam::UVec2::new(size.x, size.y + (1 << 16)),
    ] {
        assert_ne!(
            baseline,
            make(
                TextureId(0xa),
                changed_size,
                RgbaF32::WHITE,
                ImageFilter::Linear,
                ImageFilter::Linear,
            )
            .content_hash()
        );
    }
    assert_ne!(
        baseline,
        make(
            TextureId(0xa),
            size,
            RgbaF32::srgba(1.0, 0.0, 0.0, 1.0),
            ImageFilter::Linear,
            ImageFilter::Linear,
        )
        .content_hash()
    );
    assert_ne!(
        baseline,
        make(
            TextureId(0xa),
            size,
            RgbaF32::WHITE,
            ImageFilter::Nearest,
            ImageFilter::Linear,
        )
        .content_hash()
    );
    assert_ne!(
        baseline,
        make(
            TextureId(0xa),
            size,
            RgbaF32::WHITE,
            ImageFilter::Linear,
            ImageFilter::Nearest,
        )
        .content_hash()
    );
}

fn text_shape(line_height: f32, weight: FontWeight, local_origin: Option<Vec2>) -> ShapeRecord {
    text_face(
        GlyphFont {
            size: 16.0,
            line_height,
            family: FontFamily::SANS,
            weight,
            slant: FontSlant::Normal,
        },
        local_origin,
    )
}

fn text_face(font: GlyphFont, local_origin: Option<Vec2>) -> ShapeRecord {
    ShapeRecord::Text {
        local_origin,
        text: RecordedText::new(Span::default(), hash::str("hi")),
        color: RgbaF32::WHITE.to_f16(),
        font,
        wrap: TextWrap::Truncate,
        align: Align::default(),
    }
}

fn hash_shape(s: &ShapeRecord) -> u64 {
    s.content_hash().0
}

/// Pin: every authoring-relevant `ShapeRecord::Text` field participates in the
/// hash so caches invalidate on a change. New fields go in the table.
#[test]
fn text_shape_hash_distinguishes_each_authoring_field() {
    let (regular, bold) = (FontWeight::REGULAR, FontWeight::BOLD);
    let o_a = Some(Vec2::new(0.0, 0.0));
    let o_b = Some(Vec2::new(5.0, 5.0));
    let upright = GlyphFont::new(16.0);
    let cases: [(&str, ShapeRecord, ShapeRecord); 6] = [
        (
            "line_height",
            text_shape(16.0 * 1.2, regular, None),
            text_shape(16.0 * 1.5, regular, None),
        ),
        (
            "weight regular vs bold",
            text_shape(19.2, regular, None),
            text_shape(19.2, bold, None),
        ),
        (
            "local_origin None vs Some",
            text_shape(19.2, regular, None),
            text_shape(19.2, regular, o_a),
        ),
        (
            "local_origin Some(a) vs Some(b)",
            text_shape(19.2, regular, o_a),
            text_shape(19.2, regular, o_b),
        ),
        (
            "family sans vs mono",
            text_face(upright, None),
            text_face(
                GlyphFont {
                    family: FontFamily::MONO,
                    ..upright
                },
                None,
            ),
        ),
        (
            "style upright vs italic",
            text_face(upright, None),
            text_face(
                GlyphFont {
                    slant: FontSlant::Italic,
                    ..upright
                },
                None,
            ),
        ),
    ];
    for (label, a, b) in cases {
        assert_ne!(
            hash_shape(&a),
            hash_shape(&b),
            "case `{label}`: distinct fields must hash differently",
        );
    }
}

/// Identical shapes hash identically (no `RandomState`).
#[test]
fn text_shape_hash_matches_when_inputs_match() {
    assert_eq!(
        hash_shape(&text_shape(19.2, FontWeight::REGULAR, None)),
        hash_shape(&text_shape(19.2, FontWeight::REGULAR, None)),
    );
}
