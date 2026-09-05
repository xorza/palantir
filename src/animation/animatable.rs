//! Vocabulary for "things that can animate." A type is `Animatable`
//! when it supports interpolation, spring displacement arithmetic,
//! and a squared magnitude used by settle checks. Value-dependent
//! snap-only fields normalize before that arithmetic. Built-in impls
//! cover `f32`, `Vec2`, `RgbaF32`. Domain types (`Stroke`,
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
///
/// `Clone` (not `Copy`): the heavy `Animatable` types — `Background`
/// (124 B), `Brush` (60 B with inline gradient stops), `Stroke` — ride
/// the recording chain once per chromed widget per frame, where
/// auto-`Copy` costs a `vmovups` ladder per hop (~35 % self in the node
/// opener when they were `Copy`). A `Copy` supertrait here would bring
/// that back in through the bound, so duplication sites spell
/// `.clone()` and every copy stays a call-site decision; small `Copy`
/// types (`f32`, `Vec2`, `RgbaF32`) still pass through the trait at zero
/// cost because `Copy → Clone` is a no-op codegen. Both sizes are
/// pinned by `hot_struct_sizes_are_pinned`.
pub trait Animatable: Clone + PartialEq + 'static {
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

    /// Normalize fields that cannot participate in spring arithmetic.
    ///
    /// Compound derives forward this hook fieldwise. Implementations can
    /// install the target and clear only their matching velocity without
    /// disturbing independently animated sibling fields.
    #[inline]
    fn normalize_for_spring(&mut self, _target: &Self, _velocity: &mut Self) {}
}

// Written per type rather than as a blanket impl over `Add + Sub +
// Mul<f32>`: the blanket would claim every such type in the crate and
// beyond, and `Animatable` is a decision each type makes.
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

// `RgbaF32` derives `Animatable` (see `primitives/color.rs`); the
// generated impl is per-component lerp/add/sub/scale,
// sum-of-squared-component magnitude_squared, all-zeros for `zero()`.
//
// No `Option<T>` blanket: when a struct's field is "absent or value"
// (e.g. a stroke), use a sentinel value (`Stroke::ZERO`) rather
// than `Option<Stroke>` and let the paint-time `is_noop` filter
// handle the absent case. A blanket impl can only return `Some(...)`
// from arithmetic, which forces every consumer to scrub the no-op
// output back to `None` for hash equality.
