//! Point fold that derives a bounding rect, with the NaN contract.

use crate::primitives::geometry::rect::Rect;
use glam::Vec2;

/// Axis-aligned bounds of a point set, folded so a NaN cannot be lost.
///
/// `f32::min`/`max` drop a NaN operand, yielding a finite box that excludes the bad point. A
/// branch-free `is_nan` flag rides beside the SIMD fold (NaN-propagating compares measured 5x
/// slower); any NaN point yields [`Rect::NAN`], so one bbox test replaces a scan of the points.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Aabb {
    lo: Vec2,
    hi: Vec2,
    saw_nan: bool,
}

impl Aabb {
    /// Bounds of `points`, or [`Rect::ZERO`] when empty.
    #[inline]
    pub(crate) fn of(points: &[Vec2]) -> Rect {
        Self::of_iter(points.iter().copied())
    }

    #[inline]
    pub(crate) fn of_iter(points: impl IntoIterator<Item = Vec2>) -> Rect {
        let mut points = points.into_iter();
        let Some(first) = points.next() else {
            return Rect::ZERO;
        };
        let mut bounds = Self {
            lo: first,
            hi: first,
            saw_nan: first.is_nan(),
        };
        for p in points {
            bounds.push(p);
        }
        bounds.finish()
    }

    #[inline]
    fn push(&mut self, p: Vec2) {
        self.saw_nan |= p.is_nan();
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }

    #[inline]
    const fn finish(self) -> Rect {
        if self.saw_nan {
            return Rect::NAN;
        }
        Rect::from_min_max(self.lo, self.hi)
    }
}

#[cfg(test)]
mod tests {
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
}
