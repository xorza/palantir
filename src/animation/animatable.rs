//! Vocabulary for "things that can animate." A type is `Animatable`
//! when it supports linear interpolation, vector add/sub/scale, and
//! has a squared magnitude (used by spring settle checks). Built-in
//! impls cover `f32`, `Vec2`, `Color`. Domain types (`Stroke`,
//! `Background`, ...) opt in via `#[derive(Animatable)]` — see
//! `palantir-anim-derive` and the type-erased `AnimMap` storage.

use glam::Vec2;

/// Math-only trait. Storage is decoupled (type-erased `AnimMap`
/// keyed on `TypeId`), so adding a new `Animatable` type doesn't
/// require touching central code.
///
/// `PartialEq` supertrait lets `tick` short-circuit retarget
/// detection with a bytewise compare — most frames have an unchanged
/// target, so we skip the sub + magnitude pair on the steady-state
/// path. All built-in and derived types already implement
/// `PartialEq`.
pub trait Animatable: Copy + PartialEq + 'static {
    fn lerp(a: Self, b: Self, t: f32) -> Self;
    fn sub(self, other: Self) -> Self;
    fn add(self, other: Self) -> Self;
    fn scale(self, k: f32) -> Self;
    /// Squared length, compared against `EPS * EPS` for settle checks.
    /// Squared form avoids a per-frame `sqrt` for the spring termination
    /// path. For scalars: `self * self`. For vectors: dot(self, self).
    /// For derived compound types: sum of component squared magnitudes.
    fn magnitude_squared(self) -> f32;
    fn zero() -> Self;
}

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

// `Color` derives `Animatable` (see `primitives/color.rs`) — the
// generated impl is identical to the hand-written one used to live
// here; per-component lerp/add/sub/scale, sum-of-squared-component
// magnitude_squared, all-zeros for `zero()`.
//
// No `Option<T>` blanket: when a struct's field is "absent or value"
// (e.g. a stroke), use a sentinel value (`Stroke::ZERO`) rather
// than `Option<Stroke>` and let the paint-time `is_noop` filter
// handle the absent case. The blanket used to be present but
// always returned `Some(...)` from arithmetic, forcing every
// consumer to scrub the no-op output back to `None` for hash equality.
