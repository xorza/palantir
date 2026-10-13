use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::rect::aabb::Aabb;
use glam::Vec2;

/// Hand-computed bounds; any NaN yields a NaN rect; slice and stream agree.
#[test]
fn bounds_fold_points_and_any_nan_poisons_them() {
    let nan = f32::NAN;
    let cases: [(&str, Vec<Vec2>, Option<Rect>); 6] = [
        ("empty", vec![], Some(Rect::ZERO)),
        (
            "one point",
            vec![Vec2::new(3.0, 4.0)],
            Some(Rect::new(3.0, 4.0, 0.0, 0.0)),
        ),
        (
            "spread",
            vec![
                Vec2::new(3.0, 4.0),
                Vec2::new(-1.0, 9.0),
                Vec2::new(5.0, 2.0),
            ],
            Some(Rect::new(-1.0, 2.0, 6.0, 7.0)),
        ),
        (
            "nan first",
            vec![Vec2::new(nan, 0.0), Vec2::new(1.0, 1.0)],
            None,
        ),
        ("nan later x", vec![Vec2::ZERO, Vec2::new(nan, 1.0)], None),
        ("nan later y", vec![Vec2::ZERO, Vec2::new(1.0, nan)], None),
    ];
    for (label, points, want) in cases {
        let slice = Aabb::of(&points);
        let stream = Aabb::of_iter(points.iter().copied());
        if let Some(rect) = want {
            assert_eq!(slice, rect, "{label}");
            assert_eq!(stream, rect, "{label}: stream");
        } else {
            assert!(slice.has_nan(), "{label}: {slice:?}");
            assert!(stream.has_nan(), "{label}: stream {stream:?}");
        }
    }
}
