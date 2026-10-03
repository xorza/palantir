//! The point fold that derives a bounding rect, with the NaN contract that
//! keeps a bad vertex from producing a plausible box.

use crate::primitives::geometry::rect::Rect;
use glam::Vec2;

/// Axis-aligned bounds of a set of points, folded so that a NaN cannot
/// be lost. Built through [`Self::of`] / [`Self::of_iter`]; the
/// accumulator itself is an implementation detail.
///
/// **The AABB NaN contract.** `f32::min`/`max` are IEEE `minNum`/
/// `maxNum`: given a NaN they return the *other* operand. Fold with them
/// and a NaN point contributes nothing — you get a perfectly finite box
/// that simply doesn't contain it. The point still reaches the GPU, but
/// the bound damage and culling are computed from no longer covers it.
/// That is a **wrong** bound, not a missing one, and wrong bounds leave
/// trails.
///
/// So the fold keeps its fast path and carries the verdict alongside:
/// `min`/`max` stay the two-instruction SIMD form they were, and a
/// separate flag ORs in one branch-free `is_nan` per point. That is a
/// couple of ALU ops on a loop that is memory-bound over its points
/// anyway — where making the comparisons themselves NaN-propagating
/// measured **5×** slower, because the branchy selects defeat
/// vectorization outright.
///
/// The result is [`Rect::NAN`] if any point was NaN, which buys the
/// invariant every bbox-based no-op check depends on: **a NaN input
/// yields a NaN bbox.** One `O(1)` test on the derived bbox stands in
/// for an `O(n)` scan of the points behind it.
///
/// Bounds derived from a *fixed* number of inputs (a cubic's four
/// control points, an arc's centre and angles) have nothing to
/// amortize, so they screen their inputs directly and return
/// [`Rect::NAN`] rather than routing through here.
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

    /// [`Self::of`] over a point *stream* — what a mesh needs, since its
    /// positions are strided through `MeshVertex` rather than
    /// contiguous. Measured identical to a hand-rolled loop on the
    /// contiguous case, so the slice form is just this with a `copied`.
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

    /// Extend by one point. The `min`/`max` pair is the same SIMD form
    /// an unguarded fold would use; the flag is the whole added cost.
    #[inline]
    fn push(&mut self, p: Vec2) {
        self.saw_nan |= p.is_nan();
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }

    #[inline]
    fn finish(self) -> Rect {
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

    /// The fold's bounds against hand-computed rects, and the NaN
    /// contract: a NaN anywhere — first point, a later one, either lane —
    /// yields a NaN rect rather than the finite box `min`/`max` would
    /// leave. Empty input is the zero rect, and the slice and stream
    /// forms agree.
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
            match want {
                Some(rect) => {
                    assert_eq!(slice, rect, "{label}");
                    assert_eq!(stream, rect, "{label}: stream");
                }
                None => {
                    assert!(slice.has_nan(), "{label}: {slice:?}");
                    assert!(stream.has_nan(), "{label}: stream {stream:?}");
                }
            }
        }
    }
}
