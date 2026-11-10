//! Shapes that must hash apart, and the spans that must not count.

use crate::common::hash;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::math::domain::EPS;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::geometry::box_geometry::{BoxGeometry, BoxOutline};
use crate::shape::geometry::triangle_geometry::TriangleGeometry;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::record::*;
use crate::text::family_keys::FamilyKey;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;

/// Same box payload, a different covered side or outline, and both quad shapes share
/// [`ShapeRecord::Quad`]'s discriminant, so `QuadShape`'s own must separate them and each shape's
/// fields must reach the hasher. A collision makes damage diff skip a repaint.
#[test]
fn quad_shapes_hash_apart() {
    let fill = LoweredBrush::Solid(RgbaF32::WHITE.to_f16());
    let stroke = LoweredBorder::lower(Border::new(RgbaF32::BLACK, 2.0));
    let quad_box = |coverage, outline| {
        ShapeRecord::Quad(QuadShape::Box {
            geometry: BoxGeometry {
                local_rect: None,
                outline,
                coverage,
            },
            fill,
            border: stroke,
            shadow: LoweredShadow::lower(Shadow::NONE),
            material: None,
        })
    };
    let triangle = ShapeRecord::Quad(QuadShape::Triangle {
        geometry: TriangleGeometry {
            corners: [Vec2::ZERO; 3],
            radius: 0.0,
            placement: PointPlacement::NATURAL,
        },
        fill: LoweredBrush::Solid(RgbaF32::WHITE.to_f16()),
        border: stroke,
        material: None,
    });
    let sharp = BoxOutline::Rect(Corners::ZERO);
    let mut seen = Vec::new();
    for (label, record) in [
        ("inside", quad_box(Coverage::Inside, sharp)),
        ("outside", quad_box(Coverage::Outside, sharp)),
        ("circle", quad_box(Coverage::Inside, BoxOutline::Circle)),
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

/// A degenerate cubic and an arc of no sweep share their stroke, cap and ramp, so only the
/// record's discriminant tells them apart.
#[test]
fn curve_and_arc_records_hash_apart() {
    let stroke = LoweredStroke::lower(Stroke::new(RgbaF32::WHITE, 2.0));
    let curve = ShapeRecord::Curve {
        cap: LineCap::Butt,
        geometry: BezierGeometry {
            bezier: CubicBezier {
                p0: Vec2::ZERO,
                p1: Vec2::ZERO,
                p2: Vec2::ZERO,
                p3: Vec2::ZERO,
            },
            placement: PointPlacement::NATURAL,
        },
        stroke,
        ramp: CurveRamp::None,
    };
    let arc = ShapeRecord::Arc {
        cap: LineCap::Butt,
        geometry: ArcGeometry {
            local_rect: None,
            start_angle: 0.0,
            sweep: 0.0,
        },
        stroke,
        ramp: CurveRamp::None,
    };
    assert_ne!(curve.content_hash(), arc.content_hash());
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
        placement: PointPlacement::NATURAL,
        tint,
        vertices: Span::new(0, 3),
        idxs: Span::new(0, 3),
        bbox: Rect::ZERO,
        content_hash: ContentHash::from_u64(0xdead_beef),
    };
    let b = ShapeRecord::Mesh {
        placement: PointPlacement::NATURAL,
        tint,
        vertices: Span::new(1234, 3),
        idxs: Span::new(5678, 3),
        bbox: Rect::ZERO,
        content_hash: ContentHash::from_u64(0xdead_beef),
    };
    assert_eq!(a.content_hash(), b.content_hash());

    let with_rect = |rect| ShapeRecord::Mesh {
        placement: PointPlacement {
            local_rect: Some(rect),
            fit: Fit::Natural,
        },
        tint,
        vertices: Span::new(0, 3),
        idxs: Span::new(0, 3),
        bbox: Rect::ZERO,
        content_hash: ContentHash::from_u64(0xdead_beef),
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
    let image = |source| ShapeRecord::ImagePaint {
        geometry: ClosedGeometry::Box {
            local_rect: None,
            outline: BoxOutline::Rect(Corners::ZERO),
        },
        tint: RgbaF32::WHITE.to_f16(),
        source,
        fit: ImageFit::Fill,
        min_filter: ImageFilter::Linear,
        mag_filter: ImageFilter::Linear,
        downsample: ImageDownsample::Single,
    };
    let gpu_view = |id, epoch| {
        image(ImageSource::GpuView {
            id: TextureId(id),
            epoch,
        })
        .content_hash()
    };
    // The same id under the other tag, so only the source tag differs.
    let view = gpu_view(7, 7);
    assert_ne!(
        view,
        image(ImageSource::Texture {
            id: TextureId(7),
            size: glam::UVec2::ZERO,
            generation: 0,
        })
        .content_hash(),
        "a view must not collide with a texture of the same id",
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
        gpu_view(7, 8),
        "a bumped epoch must move the hash, or the view never repaints",
    );
    assert_ne!(
        view,
        gpu_view(8, 7),
        "a new target, as a replaced callback takes, must move the hash",
    );
    assert_eq!(
        view,
        gpu_view(7, 7),
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
        ShapeRecord::ImagePaint {
            geometry: ClosedGeometry::Box {
                local_rect: None,
                outline: BoxOutline::Rect(Corners::ZERO),
            },
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
        ShapingFont {
            font_size: 16.0,
            line_height,
            family: FamilyKey::from_raw(0),
            weight,
            slant: FontSlant::Normal,
        },
        local_origin,
    )
}

fn text_face(font: ShapingFont, local_origin: Option<Vec2>) -> ShapeRecord {
    ShapeRecord::Text {
        local_origin,
        text: RecordedText::new(Span::default(), hash::str("hi")),
        color: RgbaF32::WHITE.to_f16(),
        font,
        wrap: TextWrap::Truncate,
        align: Align::default(),
    }
}

fn hash_shape(s: &ShapeRecord) -> ContentHash {
    s.content_hash()
}

/// Pin: every authoring-relevant `ShapeRecord::Text` field participates in the
/// hash so caches invalidate on a change. New fields go in the table.
#[test]
fn text_shape_hash_distinguishes_each_authoring_field() {
    let (regular, bold) = (FontWeight::REGULAR, FontWeight::BOLD);
    let o_a = Some(Vec2::new(0.0, 0.0));
    let o_b = Some(Vec2::new(5.0, 5.0));
    let upright = ShapingFont {
        font_size: 16.0,
        line_height: 16.0,
        family: FamilyKey::from_raw(0),
        weight: FontWeight::REGULAR,
        slant: FontSlant::Normal,
    };
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
                ShapingFont {
                    family: FamilyKey::from_raw(1),
                    ..upright
                },
                None,
            ),
        ),
        (
            "style upright vs italic",
            text_face(upright, None),
            text_face(
                ShapingFont {
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
