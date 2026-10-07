//! Shapes that must hash apart, and the spans that must not count.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::math::domain::EPS;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::hash::compute_record_hash;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::record::*;
use crate::shape::rect::RectKind;

/// Same rectangle payload, different paint kind (a windowed rect inverts the painted region), and all three quad shapes share [`ShapeRecord::Quad`]'s discriminant, so `QuadShape`'s own must separate them and each shape's fields must reach the hasher. A collision makes damage diff skip a repaint.
#[test]
fn quad_shapes_hash_apart() {
    let fill = ShapeBrush::Solid(RgbaF16::from(RgbaF32::WHITE));
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
        fill: RgbaF16::from(RgbaF32::WHITE),
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
        let hash = compute_record_hash(&record);
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
    let arc = |center, radius, a0, a1| {
        curve(CurveBasis::Arc {
            center,
            radius,
            a0,
            a1,
        })
    };
    let baseline = arc(Vec2::ZERO, 4.0, 0.0, 1.0);

    // Every non-shared field is identical, so only `CurveBasis`'s discriminant differs.
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
    let tint = RgbaF16::from(RgbaF32 {
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

/// A view composite and a texture draw share `Image`'s discriminant, so [`ImageSource`]'s must separate them and the view's `epoch` must reach the hasher, or a bumped epoch keeps a stale texture.
#[test]
fn image_source_hashes_apart_by_source() {
    let image = |source| ShapeRecord::Image {
        local_rect: None,
        tint: RgbaF16::from(RgbaF32::WHITE),
        source,
        fit: ImageFit::Fill,
        min_filter: ImageFilter::Linear,
        mag_filter: ImageFilter::Linear,
        downsample: ImageDownsample::Single,
    };
    // Same u64-shaped payload, so only the source tag differs.
    let view = compute_record_hash(&image(ImageSource::GpuView { epoch: 7 }));
    assert_ne!(
        view,
        compute_record_hash(&image(ImageSource::Texture {
            id: TextureId(7),
            size: glam::UVec2::ZERO,
            generation: 0,
        })),
        "a texture id must not collide with an epoch of the same value",
    );
    let texture = |generation| {
        compute_record_hash(&image(ImageSource::Texture {
            id: TextureId(7),
            size: glam::UVec2::ZERO,
            generation,
        }))
    };
    assert_ne!(
        texture(0),
        texture(1),
        "a write must move the hash, or a rewritten texture never repaints",
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
                tint: RgbaF32,
                min_filter: ImageFilter,
                mag_filter: ImageFilter| {
        ShapeRecord::Image {
            local_rect: None,
            tint: RgbaF16::from(tint),
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
    let baseline = compute_record_hash(&make(
        TextureId(0xa),
        size,
        RgbaF32::WHITE,
        ImageFilter::Linear,
        ImageFilter::Linear,
    ));
    assert_ne!(
        baseline,
        compute_record_hash(&make(
            TextureId(0xb),
            size,
            RgbaF32::WHITE,
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
                RgbaF32::WHITE,
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
            RgbaF32::srgba(1.0, 0.0, 0.0, 1.0),
            ImageFilter::Linear,
            ImageFilter::Linear,
        ))
    );
    assert_ne!(
        baseline,
        compute_record_hash(&make(
            TextureId(0xa),
            size,
            RgbaF32::WHITE,
            ImageFilter::Nearest,
            ImageFilter::Linear,
        ))
    );
    assert_ne!(
        baseline,
        compute_record_hash(&make(
            TextureId(0xa),
            size,
            RgbaF32::WHITE,
            ImageFilter::Linear,
            ImageFilter::Nearest,
        ))
    );
}
