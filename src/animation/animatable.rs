//! Vocabulary for "things that can animate." A type is `Animatable`
//! when it supports linear interpolation, vector add/sub/scale, and
//! has a magnitude (used by spring settle checks). Built-in impls
//! cover `f32`, `Vec2`, `Color`. Domain types (`Stroke`,
//! `Background`, ...) opt in via `#[derive(Animatable)]` — see
//! `palantir-anim-derive` and the type-erased `AnimMap` storage.

use glam::Vec2;

/// Math-only trait. Storage is decoupled (type-erased `AnimMap`
/// keyed on `TypeId`), so adding a new `Animatable` type doesn't
/// require touching central code.
pub trait Animatable: Copy + 'static {
    fn lerp(a: Self, b: Self, t: f32) -> Self;
    fn sub(self, other: Self) -> Self;
    fn add(self, other: Self) -> Self;
    fn scale(self, k: f32) -> Self;
    /// Length used to compare against the settle threshold. For
    /// scalars: `|self|`. For vectors: Euclidean norm. For derived
    /// compound types: sqrt of sum-of-squared component magnitudes.
    fn magnitude(self) -> f32;
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
    fn magnitude(self) -> f32 {
        self.abs()
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
    fn magnitude(self) -> f32 {
        self.length()
    }
    #[inline]
    fn zero() -> Self {
        Vec2::ZERO
    }
}

/// Blanket impl for `Option<T: Animatable>`. Treats `None` as the
/// `T::zero()` sentinel (which by convention is the "invisible /
/// neutral" value for the type — transparent color, zero-width
/// stroke, etc.). The arithmetic always returns `Some(...)`; output
/// collapse back to `None` is the consumer's job (e.g. Background's
/// `is_noop` check filters invisible strokes at paint time).
///
/// This is what makes `#[derive(Animatable)]` work on structs with
/// optional sub-components like `Background.stroke: Option<Stroke>`.
impl<T: Animatable> Animatable for Option<T> {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Some(T::lerp(
            a.unwrap_or_else(T::zero),
            b.unwrap_or_else(T::zero),
            t,
        ))
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        Some(T::sub(
            self.unwrap_or_else(T::zero),
            other.unwrap_or_else(T::zero),
        ))
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        Some(T::add(
            self.unwrap_or_else(T::zero),
            other.unwrap_or_else(T::zero),
        ))
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        Some(T::scale(self.unwrap_or_else(T::zero), k))
    }
    #[inline]
    fn magnitude(self) -> f32 {
        T::magnitude(self.unwrap_or_else(T::zero))
    }
    #[inline]
    fn zero() -> Self {
        Some(T::zero())
    }
}

// `Color` derives `Animatable` (see `primitives/color.rs`) — the
// generated impl is identical to the hand-written one used to live
// here; per-component lerp/add/sub/scale, sqrt-of-sum-of-squares
// magnitude, all-zeros for `zero()`.
