use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::geometry::stroke_geometry::{PolylineColorSpan, StrokeGeometry};
use crate::shape::image_paint::ImagePaint;
use crate::shape::internals;
use crate::shape::paint::closed_paint::ClosedPaint;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::polyline::PolylineShape;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed::DrawWith;
use crate::shape::shapes::Shapes;
use glam::UVec2;
use glam::Vec2;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Clone, Copy, Debug)]
enum ColorSource {
    Single,
    PerPoint,
    PerSegment,
}

impl ColorSource {
    fn apply<'a>(self, shape: PolylineShape<'a>, colors: &'a [RgbaF32]) -> PolylineShape<'a> {
        match self {
            Self::Single => shape,
            Self::PerPoint => shape.point_colors(colors),
            Self::PerSegment => shape.segment_colors(colors),
        }
    }

    fn accepts(self, points_len: usize, colors_len: usize) -> bool {
        match self {
            Self::Single => true,
            Self::PerPoint => colors_len == points_len,
            Self::PerSegment => colors_len == points_len.saturating_sub(1),
        }
    }

    fn stored_colors_len(self, points_len: usize) -> u32 {
        match self {
            Self::Single => 0,
            Self::PerPoint => points_len as u32,
            Self::PerSegment => points_len.saturating_sub(1) as u32,
        }
    }
}

/// The color-cardinality contract is checked by the color setter, whatever the point count; an
/// accepted polyline under two points is dropped, and one of two or more stages its colors as
/// authored, none for one color, and keeps the stroke's color, which multiplies them where they
/// are drawn.
#[test]
fn polyline_color_cardinality_is_enforced_at_the_setter() {
    let points = [Vec2::ZERO, Vec2::new(10.0, 10.0)];
    let tint = RgbaF32::new(0.5, 0.25, 1.0, 0.5);
    let colors = [
        RgbaF32::new(1.0, 1.0, 1.0, 1.0),
        RgbaF32::new(0.5, 1.0, 0.5, 1.0),
        RgbaF32::new(0.0, 0.5, 1.0, 0.5),
    ];

    for points_len in 0..=2 {
        for source in [
            ColorSource::Single,
            ColorSource::PerPoint,
            ColorSource::PerSegment,
        ] {
            let color_lengths: &[usize] = match source {
                ColorSource::Single => &[0],
                ColorSource::PerPoint | ColorSource::PerSegment => &[0, 1, 2, 3],
            };

            for &colors_len in color_lengths {
                let shape = catch_unwind(AssertUnwindSafe(|| {
                    source.apply(
                        Shape::polyline(&points[..points_len]),
                        &colors[..colors_len],
                    )
                }));
                let accepted = source.accepts(points_len, colors_len);
                assert_eq!(
                    shape.is_ok(),
                    accepted,
                    "{source:?}, points_len={points_len}, colors_len={colors_len}",
                );
                let Ok(shape) = shape else {
                    continue;
                };

                let mut shapes = Shapes::default();
                let mut store = RecordStore::default();
                let stored = points_len >= 2;
                assert_eq!(
                    add(&mut shapes, (shape, Stroke::new(tint, 1.0)), &mut store),
                    stored.then_some(0)
                );
                assert_eq!(shapes.records.len(), usize::from(stored));
                assert_eq!(shapes.hashes.len(), usize::from(stored));
                assert_eq!(
                    store.polyline_points.len(),
                    points_len * usize::from(stored)
                );
                assert_eq!(
                    store.polyline_colors.len(),
                    source.stored_colors_len(points_len) as usize * usize::from(stored),
                );

                if stored {
                    let ShapeRecord::Stroke {
                        geometry: StrokeGeometry::Polyline(polyline),
                        stroke,
                    } = &shapes.records[0]
                    else {
                        panic!("accepted polyline lowered to another record variant");
                    };
                    assert_eq!(polyline.points.len, points_len as u32);
                    assert_eq!(stroke.color, tint.to_f16());
                    let staged = match (source, polyline.colors) {
                        (ColorSource::Single, PolylineColorSpan::Single) => &[][..],
                        (ColorSource::PerPoint, PolylineColorSpan::PerPoint(span))
                        | (ColorSource::PerSegment, PolylineColorSpan::PerSegment(span)) => {
                            &store.polyline_colors[span.range()]
                        }
                        (source, colors) => panic!("{source:?} staged as {colors:?}"),
                    };
                    let expected: Vec<RgbaF16> =
                        colors[..colors_len].iter().map(|&c| c.to_f16()).collect();
                    assert_eq!(staged, expected.as_slice(), "{source:?}");
                }
            }
        }
    }
}

#[test]
fn image_dimensions_above_u16_survive_lowering() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let mut shapes = Shapes::default();
    let mut store = RecordStore::default();

    assert_eq!(
        add(
            &mut shapes,
            (
                Shape::rect(),
                ImagePaint::new(internals::image_handle(WIDTH))
            ),
            &mut store
        ),
        Some(0)
    );
    let ShapeRecord::Closed {
        paint:
            ClosedPaint::Image {
                source: ImageSource::Texture { size, .. },
                ..
            },
        ..
    } = shapes.records[0]
    else {
        panic!("image lowered to another record variant or source");
    };
    assert_eq!(size, UVec2::new(WIDTH, 1));
}

/// An image keeps its rect's corners, its circle or its triangle in the record, which clip it.
#[test]
fn an_image_keeps_the_shape_that_clips_it() {
    use crate::primitives::geometry::corners::Corners;
    use crate::shape::geometry::box_geometry::BoxGeometry;
    use crate::shape::geometry::box_geometry::BoxOutline;
    use crate::shape::geometry::closed_geometry::ClosedGeometry;
    use crate::shape::geometry::point_placement::PointPlacement;
    use crate::shape::geometry::triangle_geometry::TriangleGeometry;

    let mut shapes = Shapes::default();
    let mut store = RecordStore::default();
    let paint_of = |side| ImagePaint::new(internals::image_handle(side));
    let rounded = Shape::rect().corners(Corners::new(1.0, 2.0, 3.0, 4.0));
    let paint = paint_of(4);
    let _ = internals::check(&rounded, paint.clone());
    assert_eq!(add(&mut shapes, (rounded, paint), &mut store), Some(0));
    let ShapeRecord::Closed {
        geometry,
        paint: ClosedPaint::Image { .. },
    } = shapes.records[0]
    else {
        panic!("an image lowers to an image record");
    };
    assert_eq!(
        geometry,
        ClosedGeometry::Box(BoxGeometry {
            local_rect: None,
            outline: BoxOutline::Rect(Corners::new(1.0, 2.0, 3.0, 4.0)),
        })
    );
    // A triangle takes an image, its geometry kept for the draw to place; one of no area paints
    // nothing.
    let corners = [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(0.0, 4.0)];
    let triangle = Shape::triangle(corners[0], corners[1], corners[2]).radius(1.0);
    let _ = internals::check(&triangle, paint_of(4));
    assert_eq!(
        add(&mut shapes, (triangle, paint_of(4)), &mut store),
        Some(1)
    );
    let ShapeRecord::Closed {
        geometry,
        paint: ClosedPaint::Image { .. },
    } = shapes.records[1]
    else {
        panic!("a triangle's image lowers to an image record");
    };
    assert_eq!(
        geometry,
        ClosedGeometry::Triangle(TriangleGeometry {
            corners,
            radius: 1.0,
            placement: PointPlacement::NATURAL,
        })
    );
    let flat = Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0));
    assert_eq!(add(&mut shapes, (flat, paint_of(4)), &mut store), None);
    // A circle takes an image as a round box; one at a rect of no size paints nothing.
    let at = Rect::new(1.0, 2.0, 8.0, 4.0);
    let circle = Shape::circle().at(at);
    let _ = internals::check(&circle, paint_of(4));
    assert_eq!(add(&mut shapes, (circle, paint_of(4)), &mut store), Some(2));
    let ShapeRecord::Closed {
        geometry,
        paint: ClosedPaint::Image { .. },
    } = shapes.records[2]
    else {
        panic!("a circle's image lowers to an image record");
    };
    assert_eq!(
        geometry,
        ClosedGeometry::Box(BoxGeometry {
            local_rect: Some(at),
            outline: BoxOutline::Circle,
        })
    );
    let empty = Shape::circle().at(Rect::new(1.0, 2.0, 0.0, 4.0));
    assert_eq!(add(&mut shapes, (empty, paint_of(4)), &mut store), None);
}

/// Add a shape and its paint to `shapes`, as `Ui::add_shape` records it ([`internals::record`]).
fn add<S: DrawWith<P>, P>(
    shapes: &mut Shapes,
    pair: (S, P),
    store: &mut RecordStore,
) -> Option<u32> {
    internals::record(store, pair).map(|record| shapes.push(record))
}

/// A shadow the crate derives past the lanes, as an animated chrome's tween overshoots, saturates
/// where it lowers: 65520, the least magnitude that would pack to infinity, lowers to 65504, and
/// its halo stays finite.
#[test]
fn a_derived_shadow_past_65504_saturates_where_it_lowers() {
    use crate::primitives::paint::shadow::Shadow;
    use crate::shape::paint::lowered_shadow::LoweredShadow;

    let geometry = LoweredShadow::lower(Shadow {
        offset: Vec2::new(-65520.0, 1.0),
        blur: 65520.0,
        spread: 1.0e9,
        ..Shadow::default()
    })
    .geometry();
    assert_eq!(
        (geometry.offset, geometry.blur, geometry.spread),
        (Vec2::new(-65504.0, 1.0), 65504.0, 65504.0)
    );
    assert_eq!(geometry.halo(), 4.0 * 65504.0 + 65504.0);
}
