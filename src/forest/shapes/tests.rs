use crate::forest::shapes::Shapes;
use crate::forest::shapes::record::ShapeRecord;
use crate::primitives::color::Color;
use crate::record_store::RecordStore;
use crate::shape::{PolylineColors, Shape};
use glam::Vec2;
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
                let result = catch_unwind(AssertUnwindSafe(|| shapes.add(shape, &store)));
                let accepted = source.accepts(points_len, colors_len);

                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "{source:?}, points_len={points_len}, colors_len={colors_len}",
                );

                if !accepted {
                    assert!(shapes.records.is_empty());
                    assert!(shapes.hashes.is_empty());
                    let payloads = store.borrow();
                    assert!(payloads.polyline_points.is_empty());
                    assert!(payloads.polyline_colors.is_empty());
                    continue;
                }

                let stored = points_len == 2;
                assert_eq!(result.unwrap(), stored.then_some(0));
                assert_eq!(shapes.records.len(), usize::from(stored));
                assert_eq!(shapes.hashes.len(), usize::from(stored));
                let payloads = store.borrow();
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
