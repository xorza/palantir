//! One row or column of a grid, and the interned definition a node carries
//! a whole grid by.

use crate::common::span::Span;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::{self, FloatHash};
use std::hash;

/// One row or column definition for a `Grid`: a `Sizing` with `[min, max]` clamps.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct Track {
    pub(crate) size: Sizing,
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl Track {
    /// This track's Hug floor: min-content raised to `min` and capped at `max`; one place so measure, arrange and intrinsic agree.
    #[inline]
    pub(crate) const fn content_floor(&self, min_content: f32) -> f32 {
        min_content.max(self.min).min(self.max)
    }

    /// A track with the given sizing.
    pub const fn new(size: Sizing) -> Self {
        Self {
            size,
            min: 0.0,
            max: f32::INFINITY,
        }
    }

    /// The four constructors mirror [`Sizing`]'s name for name.
    pub const HUG: Self = Self::new(Sizing::HUG);

    /// A track taking an equal share of the leftover; [`Self::fill`]
    /// weights it.
    pub const FILL: Self = Self::new(Sizing::FILL);

    /// [`Sizing::fixed`] as a track.
    ///
    /// # Panics
    ///
    /// As [`Sizing::fixed`].
    #[track_caller]
    pub const fn fixed(v: f32) -> Self {
        Self::new(Sizing::fixed(v))
    }

    /// [`Sizing::fill`] as a track.
    ///
    /// # Panics
    ///
    /// As [`Sizing::fill`].
    #[track_caller]
    pub const fn fill(weight: f32) -> Self {
        Self::new(Sizing::fill(weight))
    }

    /// Set the lower size clamp (a *length*); a maximum below it is raised.
    ///
    /// # Panics
    ///
    /// Panics unless `min` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn with_min(mut self, min: f32) -> Self {
        self.min = domain::length(min);
        self.max = self.max.max(min);
        self
    }

    /// Set the upper size clamp (an *extent*; `+inf` is unbounded); raised to the minimum if below it.
    ///
    /// # Panics
    ///
    /// Panics unless `max` is an [extent](crate::widget::domain::extent).
    #[track_caller]
    pub const fn with_max(mut self, max: f32) -> Self {
        self.max = domain::extent(max).max(self.min);
        self
    }

    /// One `u64` for the two clamps, as [`FloatHash`] does for [`glam::Vec2`].
    #[inline]
    fn hash_bits<H: hash::Hasher, F: Fn(f32) -> u32 + Copy>(&self, h: &mut H, bits: F) {
        self.size.hash_bits(h, bits);
        h.write_u64((u64::from(bits(self.min)) << 32) | u64::from(bits(self.max)));
    }
}

impl From<Sizing> for Track {
    fn from(s: Sizing) -> Self {
        Self::new(s)
    }
}

impl FloatHash for Track {
    #[inline]
    fn hash_eq<H: hash::Hasher>(&self, h: &mut H) {
        self.hash_bits(h, float_hash::eq_bits);
    }

    #[inline]
    fn hash_visual<H: hash::Hasher>(&self, h: &mut H) {
        self.hash_bits(h, float_hash::canon_bits);
    }
}

impl hash::Hash for Track {
    #[inline]
    fn hash<H: hash::Hasher>(&self, h: &mut H) {
        self.hash_eq(h);
    }
}

/// Spans into a `Tree`'s retained flat track arena plus the gaps for one Grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridDef {
    pub(crate) rows: Span,
    pub(crate) cols: Span,
}

impl GridDef {
    pub(crate) fn hash_visual<H: hash::Hasher>(&self, tracks: &[Track], h: &mut H) {
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
