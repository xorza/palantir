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
mod tests;
