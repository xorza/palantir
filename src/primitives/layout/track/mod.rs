//! One row or column of a grid.

use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::{self, FloatHash};
use std::hash;

/// One row or column definition for a `Grid`: a `Sizing` with `[min, max]` clamps.
///
/// A [`Self::fixed`] track asks for its size, and its `min` starts at that size, so it stays
/// exact. Lowered with [`Self::with_min`], the `min` is how far it gives way when its line is
/// short of room for the other tracks' floors, as a [`Self::HUG`] track gives way toward its
/// content floor: a sidebar of a set width shrinks only when the window cannot also hold the
/// pane beside it at its least, as CSS's `flex: 0 1 <width>` beside a `min-width`
/// (<https://developer.mozilla.org/en-US/docs/Web/CSS/flex-shrink>).
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct Track {
    sizing: Sizing,
    min: f32,
    max: f32,
}

impl Track {
    /// This track's Hug floor: min-content raised to `min` and capped at `max`; one place so measure, arrange and intrinsic agree.
    #[inline]
    pub(crate) const fn content_floor(&self, min_content: f32) -> f32 {
        min_content.max(self.min).min(self.max)
    }

    /// A track with the given sizing: unclamped, or for a fixed size pinned at it from below.
    pub const fn new(sizing: Sizing) -> Self {
        let min = match sizing.fixed_value() {
            Some(value) => value,
            None => 0.0,
        };
        Self {
            sizing,
            min,
            max: f32::INFINITY,
        }
    }

    /// [`Sizing::HUG`] as a track; the four constructors mirror [`Sizing`]'s name for name.
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
    /// Panics unless `min` is a [length](crate::widget::domain::is_length).
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
    /// Panics unless `max` is an [extent](crate::widget::domain::is_extent).
    #[track_caller]
    pub const fn with_max(mut self, max: f32) -> Self {
        self.max = domain::extent(max).max(self.min);
        self
    }

    /// How the track sizes before its clamps.
    pub const fn sizing(self) -> Sizing {
        self.sizing
    }

    /// The lower size clamp, a *length*: unless set, `0`, or a fixed track's size.
    pub const fn min(self) -> f32 {
        self.min
    }

    /// The upper size clamp, an *extent* no smaller than [`Self::min`]; `+inf` unless set.
    pub const fn max(self) -> f32 {
        self.max
    }

    /// A fixed track's size, its value within its clamps, and `None` for another sizing.
    #[inline]
    pub(crate) fn fixed_size(self) -> Option<f32> {
        self.sizing
            .fixed_value()
            .map(|value| value.clamp(self.min, self.max))
    }

    /// Whether this is a fixed track that gives way toward its `min`: one whose `min` sits below
    /// its size.
    #[inline]
    pub(crate) fn yields(self) -> bool {
        self.fixed_size().is_some_and(|size| self.min < size)
    }

    /// One `u64` for the two clamps, as [`FloatHash`] does for [`glam::Vec2`].
    #[inline]
    fn hash_bits<H: hash::Hasher, F: Fn(f32) -> u32 + Copy>(&self, h: &mut H, bits: F) {
        self.sizing.hash_bits(h, bits);
        h.write_u64((u64::from(bits(self.min)) << 32) | u64::from(bits(self.max)));
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

#[cfg(test)]
mod tests;
