//! Vocabulary for "things that can animate." An `Animatable` type supports
//! interpolation, spring displacement arithmetic, and a squared distance in its
//! own settle tolerance; value-dependent snap-only fields normalize first.
//! Built-in impls cover `f32`, `Vec2`, `RgbaF32`; domain types opt in via
//! `#[derive(Animatable)]` (`palantir-anim-derive`, type-erased `AnimMap`).

use crate::primitives::math::domain::EPS;
use glam::Vec2;

/// Math-only trait; storage is the type-erased `AnimMap` keyed on `TypeId`.
///
/// `PartialEq` lets `tick` skip the sub + magnitude pair when the target is
/// unchanged.
///
/// `Clone`, not `Copy`: heavy types (`Background`, `Brush`, `Stroke`) ride the
/// recording chain once per chromed widget per frame, where auto-`Copy` was
/// measurably costly. A `Copy` supertrait would bring that back, so copies
/// spell `.clone()`; small `Copy` types still cost nothing. Sizes are pinned
/// by `hot_struct_sizes_are_pinned`.
pub trait Animatable: Clone + PartialEq + 'static {
    /// Interpolate from `a` to `b` at phase `t`, normally `0.0..=1.0`. A curve may
    /// overshoot, so implementations must not clamp.
    fn lerp(a: Self, b: Self, t: f32) -> Self;
    #[must_use]
    /// Difference `self - other`.
    fn sub(self, other: Self) -> Self;
    /// Componentwise sum, the inverse of [`Self::sub`].
    #[must_use]
    fn add(self, other: Self) -> Self;
    /// Componentwise multiplication by a scalar.
    #[must_use]
    fn scale(self, k: f32) -> Self;
    /// Squared Euclidean length in the type's own unit (`self * self` for
    /// scalars, `dot(self, self)` for vectors, summed components for derives).
    fn magnitude_squared(self) -> f32;
    /// Squared length in units of this type's settle tolerance: under `1.0` a
    /// displacement is too small to see and motion may end on its target.
    ///
    /// **Unit-free by default:** an `f32` may be pixels or a 0..1 fraction, so the
    /// default divides [`Self::magnitude_squared`] by `EPS²` (`EPS = 1e-4`). A type
    /// that knows its unit says so: `RgbaF32` settles at `1/4096`, under one 8-bit
    /// sRGB step near black.
    ///
    /// The derive sums its fields' own distances, so pixel and colour fields keep
    /// their own tolerances. A derive with one unit of its own uses
    /// `#[animate(settle_eps = ...)]`.
    #[inline]
    fn settle_distance_squared(self) -> f32 {
        self.magnitude_squared() / (EPS * EPS)
    }
    /// The additive identity: zero displacement and a fresh spring's velocity.
    fn zero() -> Self;

    /// Normalize fields that cannot participate in spring arithmetic.
    ///
    /// Runs when a spring takes a new target or a row becomes a spring. Between
    /// those the value is `target.add(offset)`, so an [`Self::add`] that keeps
    /// `self` for such a field keeps it compatible. Compound derives forward this
    /// fieldwise; implementations may install the target and clear only their
    /// matching velocity.
    #[inline]
    fn normalize_for_spring(&mut self, _target: &Self, _velocity: &mut Self) {}
}

// Per type, not a blanket over `Add + Sub + Mul<f32>`, which would claim every
// such type: `Animatable` is each type's decision.
impl Animatable for f32 {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        self - other
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        self + other
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        self * k
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        self * self
    }
    #[inline]
    fn zero() -> Self {
        0.0
    }
}

impl Animatable for Vec2 {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        self - other
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        self + other
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        self * k
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        self.length_squared()
    }
    #[inline]
    fn zero() -> Self {
        Vec2::ZERO
    }
}

// `RgbaF32` derives `Animatable` (see `primitives/paint/color/mod.rs`).
//
// No `Option<T>` blanket: for "absent or value" fields use a sentinel
// (`Stroke::NONE`) and let the paint-time `is_noop` filter handle absence. A
// blanket could only return `Some(...)`, forcing consumers to scrub no-ops
// back to `None` for hash equality.
