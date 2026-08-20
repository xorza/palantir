//! Shapes that must hash apart, and the spans that must not count.

use crate::primitives::approx::EPS;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::primitives::texture_id::TextureId;
use crate::scene::shapes::hash::compute_record_hash;
use crate::scene::shapes::paint::{LoweredShadow, ShapeStroke};
use crate::scene::shapes::record::*;
use crate::shape::rect::RectKind;
use glam::Vec2;

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
