use crate::primitives::rect::Rect;
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
