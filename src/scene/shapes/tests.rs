use crate::primitives::color::Color;
use crate::primitives::image::Image;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id_source::TextureIdSource;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::Shapes;
use crate::scene::shapes::paint::ImageSource;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use glam::Vec2;
use std::num::NonZeroU32;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Clone, Copy, Debug)]
enum ColorSource {
    Single,
    PerPoint,
    PerSegment,
}

impl ColorSource {
    fn colors<'a>(self, colors: &'a [Color]) -> PolylineColors<'a> {
        match self {
            ColorSource::Single => PolylineColors::Single(Color::WHITE),
            ColorSource::PerPoint => PolylineColors::PerPoint(colors),
            ColorSource::PerSegment => PolylineColors::PerSegment(colors),
        }
    }

    fn accepts(self, points_len: usize, colors_len: usize) -> bool {
        match self {
            ColorSource::Single => true,
            ColorSource::PerPoint => colors_len == points_len,
            ColorSource::PerSegment => colors_len == points_len.saturating_sub(1),
        }
    }

    fn stored_colors_len(self, points_len: usize) -> u32 {
        match self {
            ColorSource::Single => 1,
            ColorSource::PerPoint => points_len as u32,
            ColorSource::PerSegment => points_len.saturating_sub(1) as u32,
        }
    }
}

#[test]
fn polyline_color_cardinality_is_enforced_before_noop_lowering() {
    let points = [Vec2::ZERO, Vec2::new(10.0, 10.0)];
    let colors = [Color::WHITE; 3];

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
                let mut shapes = Shapes::default();
                let store = RecordStore::default();
                let shape = Shape::polyline(
                    &points[..points_len],
                    source.colors(&colors[..colors_len]),
                    1.0,
                );
                let result = catch_unwind(AssertUnwindSafe(|| shapes.add(shape.into(), &store)));
                let accepted = source.accepts(points_len, colors_len);

                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "{source:?}, points_len={points_len}, colors_len={colors_len}",
                );

                if !accepted {
                    assert!(shapes.records.is_empty());
                    assert!(shapes.hashes.is_empty());
                    let payloads = store.payloads.borrow();
                    assert!(payloads.polyline_points.is_empty());
                    assert!(payloads.polyline_colors.is_empty());
                    continue;
                }

                let stored = points_len == 2;
                assert_eq!(result.unwrap(), stored.then_some(0));
                assert_eq!(shapes.records.len(), usize::from(stored));
                assert_eq!(shapes.hashes.len(), usize::from(stored));
                let payloads = store.payloads.borrow();
                assert_eq!(
                    payloads.polyline_points.len(),
                    points_len * usize::from(stored)
                );
                assert_eq!(
                    payloads.polyline_colors.len(),
                    source.stored_colors_len(points_len) as usize * usize::from(stored),
                );

                if stored {
                    let ShapeRecord::Polyline {
                        points: point_span,
                        colors: color_span,
                        ..
                    } = &shapes.records[0]
                    else {
                        panic!("accepted polyline lowered to another record variant");
                    };
                    assert_eq!(point_span.len, points_len as u32);
                    assert_eq!(color_span.len, source.stored_colors_len(points_len));
                }
            }
        }
    }
}

#[test]
fn image_dimensions_above_u16_survive_lowering() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let registry = ImageRegistry::new(
        TextureIdSource::default(),
        Some(NonZeroU32::new(WIDTH).unwrap()),
    );
    let handle = registry
        .register(Image::from_rgba8(WIDTH, 1, vec![0; WIDTH as usize * 4]))
        .unwrap();
    let mut shapes = Shapes::default();
    let store = RecordStore::default();

    assert_eq!(shapes.add(Shape::image(handle).into(), &store), Some(0));
    let ShapeRecord::Image {
        source: ImageSource::Texture { size, .. },
        ..
    } = shapes.records[0]
    else {
        panic!("image lowered to another record variant or source");
    };
    assert_eq!(size, glam::UVec2::new(WIDTH, 1));
}

/// The NaN screen has to run *before* `is_noop`, because `noop_f32`
/// classifies NaN as invisible: a NaN-width shape reads as a no-op and
/// would leave through that early return without ever being looked at.
/// Ordering the two the other way round is a silent regression — the
/// assert still exists, it just stops seeing the case it exists for.
///
/// Debug-only by design (see the `NanCheck` module doc), so this test
/// only means anything in a `debug_assertions` build.
#[test]
#[cfg(debug_assertions)]
fn nan_is_rejected_before_the_noop_early_return() {
    let clean = |width| {
        let mut shapes = Shapes::default();
        let store = RecordStore::default();
        let shape = Shape::line(Vec2::ZERO, Vec2::new(4.0, 0.0), width).brush(Color::WHITE);
        catch_unwind(AssertUnwindSafe(|| shapes.add(shape.into(), &store))).map(|r| r.is_some())
    };

    assert!(
        clean(2.0).expect("a live shape must not panic"),
        "sanity: the fixture records without the NaN",
    );
    // Zero width is a plain no-op: dropped quietly, no panic. This is
    // the door the NaN case must not be able to use.
    assert!(
        !clean(0.0).expect("a zero-width shape is a no-op, not a panic"),
        "a zero-width shape is dropped, not recorded",
    );
    assert!(
        clean(f32::NAN).is_err(),
        "a NaN width must assert, not slip out through the no-op gate",
    );
}
