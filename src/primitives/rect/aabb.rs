use crate::primitives::rect::Rect;
use glam::Vec2;

/// Accumulator for an axis-aligned bounding box that must not lose a
/// NaN.
///
/// **The AABB NaN contract.** `f32::min`/`max` are IEEE `minNum`/
/// `maxNum`: given a NaN they return the *other* operand. Fold with them
/// and a NaN vertex contributes nothing — you get a perfectly finite box
/// that simply doesn't contain it. The point still reaches the GPU, but
/// the bound damage and culling are computed from no longer covers it.
/// That is a **wrong** bound, not a missing one, and wrong bounds leave
/// trails.
///
/// So the fold keeps its fast path and carries the verdict alongside:
/// `min`/`max` stay the two-instruction SIMD form they were, and a
/// separate `saw_nan` flag ORs in one branch-free `is_nan` per point.
/// That is a couple of ALU ops on a loop that is memory-bound over its
/// vertices anyway — where making the comparisons themselves
/// NaN-propagating would cost four branchy scalar selects per point.
///
/// [`Self::finish`] then poisons the whole rect if the flag is set,
/// which buys the invariant every bbox-based no-op check depends on:
/// **a NaN input yields a NaN bbox.** One `O(1)` test on the derived
/// bbox stands in for an `O(n)` scan of the points behind it.
///
/// For bounds derived from a *fixed* number of inputs (a cubic's four
/// control points, an arc's centre and angles) there is nothing to
/// amortize — those screen their inputs directly and return
/// [`Rect::NAN`], rather than routing through this.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Aabb {
    lo: Vec2,
    hi: Vec2,
    saw_nan: bool,
}

impl Aabb {
    /// Seed the fold with its first point.
    #[inline]
    pub(crate) fn new(first: Vec2) -> Self {
        Self {
            lo: first,
            hi: first,
            saw_nan: first.is_nan(),
        }
    }

    /// Extend by one point. The `min`/`max` pair is the same SIMD form
    /// an unguarded fold would use; the flag is the whole added cost.
    #[inline]
    pub(crate) fn push(&mut self, p: Vec2) {
        self.saw_nan |= p.is_nan();
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }

    /// The bounding rect, or [`Rect::NAN`] if any point was NaN.
    #[inline]
    pub(crate) fn finish(self) -> Rect {
        if self.saw_nan {
            return Rect::NAN;
        }
        Rect::from_min_max(self.lo, self.hi)
    }
}
