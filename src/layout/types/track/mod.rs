//! One row or column of a grid, and the interned definition a node carries
//! a whole grid by.

use crate::layout::types::limits::{valid_lower_bound, valid_upper_bound};
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::FloatHash;
use crate::primitives::span::Span;

/// One row or column definition for a `Grid`. Wraps a `Sizing` (Pixel / Auto /
/// Star) with optional `[min, max]` clamps. Defaults: `min = 0.0`,
/// `max = INFINITY` (no clamp).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub(crate) size: Sizing,
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl Track {
    /// This track's Hug floor: its content's min-content extent, raised
    /// to the track's own `min` and capped at its `max`. The one place
    /// the three bounds are combined, so the measure solve, the arrange
    /// solve, and the intrinsic aggregator cannot disagree about which
    /// wins.
    #[inline]
    pub(crate) fn content_floor(&self, min_content: f32) -> f32 {
        min_content.max(self.min).min(self.max)
    }

    pub const fn new(size: Sizing) -> Self {
        Self {
            size,
            min: 0.0,
            max: f32::INFINITY,
        }
    }

    /// The four constructors mirror [`Sizing`]'s name for name — `HUG`,
    /// `FILL`, `fixed`, `fill` — because a track *is* a sizing plus two
    /// clamps, and one vocabulary is what keeps `fill` from meaning a
    /// constructor here and an accessor there.
    pub const HUG: Self = Self::new(Sizing::HUG);

    /// A track taking an equal share of the leftover; [`Self::fill`]
    /// weights it.
    pub const FILL: Self = Self::new(Sizing::FILL);

    pub const fn fixed(v: f32) -> Self {
        Self::new(Sizing::fixed(v))
    }

    pub const fn fill(weight: f32) -> Self {
        Self::new(Sizing::fill(weight))
    }

    /// Set the lower size clamp.
    ///
    /// # Panics
    ///
    /// Panics if `min` is negative, non-finite, or greater than the current
    /// maximum.
    pub const fn min(mut self, min: f32) -> Self {
        assert!(
            valid_lower_bound(min) && min <= self.max,
            "Track minimum must be finite, non-negative, and not exceed its maximum",
        );
        self.min = min;
        self
    }

    /// Set the upper size clamp.
    ///
    /// # Panics
    ///
    /// Panics if `max` is negative, NaN, or less than the current minimum.
    /// Positive infinity is the unbounded sentinel.
    pub const fn max(mut self, max: f32) -> Self {
        assert!(
            valid_upper_bound(max) && max >= self.min,
            "Track maximum must be non-negative and not be less than its minimum",
        );
        self.max = max;
        self
    }

    #[inline]
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, h: &mut H) {
        self.size.hash_visual(h);
        self.min.hash_visual(h);
        self.max.hash_visual(h);
    }
}

impl From<Sizing> for Track {
    fn from(s: Sizing) -> Self {
        Self::new(s)
    }
}

impl std::hash::Hash for Track {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.size.hash(h);
        self.min.hash_eq(h);
        self.max.hash_eq(h);
    }
}

/// Spans into a `Tree`'s retained flat track arena plus the gaps for one Grid.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GridDef {
    pub(crate) rows: Span,
    pub(crate) cols: Span,
}

impl GridDef {
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, tracks: &[Track], h: &mut H) {
        h.write_u32(self.rows.len);
        for t in &tracks[self.rows.range()] {
            t.hash_visual(h);
        }
        h.write_u32(self.cols.len);
        for t in &tracks[self.cols.range()] {
            t.hash_visual(h);
        }
    }
}

#[cfg(test)]
mod tests;
