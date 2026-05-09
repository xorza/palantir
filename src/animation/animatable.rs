//! Vocabulary for "things that can animate." A type is `Animatable`
//! when it supports linear interpolation, vector add/sub/scale, and
//! has a magnitude (used by spring settle checks). Implemented for
//! `f32`, `Vec2`, `Color`. Add a new type by implementing the trait
//! and adding a typed slot to [`AnimMap`].

use crate::animation::{AnimMap, AnimMapTyped};
use crate::primitives::color::Color;
use glam::Vec2;

pub trait Animatable: Copy + 'static {
    fn lerp(a: Self, b: Self, t: f32) -> Self;
    fn sub(self, other: Self) -> Self;
    fn add(self, other: Self) -> Self;
    fn scale(self, k: f32) -> Self;
    /// Length used to compare against the settle threshold. For
    /// scalars: `|self|`. For vectors: Euclidean norm.
    fn magnitude(self) -> f32;
    fn zero() -> Self;
    /// Per-type slot in the central [`AnimMap`]. Lets `Ui::animate` be
    /// generic over `T` without runtime type-erasure.
    fn slot_mut(am: &mut AnimMap) -> &mut AnimMapTyped<Self>;
}

impl Animatable for f32 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
    fn sub(self, other: Self) -> Self {
        self - other
    }
    fn add(self, other: Self) -> Self {
        self + other
    }
    fn scale(self, k: f32) -> Self {
        self * k
    }
    fn magnitude(self) -> f32 {
        self.abs()
    }
    fn zero() -> Self {
        0.0
    }
    fn slot_mut(am: &mut AnimMap) -> &mut AnimMapTyped<Self> {
        &mut am.scalars
    }
}

impl Animatable for Vec2 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
    fn sub(self, other: Self) -> Self {
        self - other
    }
    fn add(self, other: Self) -> Self {
        self + other
    }
    fn scale(self, k: f32) -> Self {
        self * k
    }
    fn magnitude(self) -> f32 {
        self.length()
    }
    fn zero() -> Self {
        Vec2::ZERO
    }
    fn slot_mut(am: &mut AnimMap) -> &mut AnimMapTyped<Self> {
        &mut am.vec2s
    }
}

impl Animatable for Color {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Color {
            r: a.r + (b.r - a.r) * t,
            g: a.g + (b.g - a.g) * t,
            b: a.b + (b.b - a.b) * t,
            a: a.a + (b.a - a.a) * t,
        }
    }
    fn sub(self, other: Self) -> Self {
        Color {
            r: self.r - other.r,
            g: self.g - other.g,
            b: self.b - other.b,
            a: self.a - other.a,
        }
    }
    fn add(self, other: Self) -> Self {
        Color {
            r: self.r + other.r,
            g: self.g + other.g,
            b: self.b + other.b,
            a: self.a + other.a,
        }
    }
    fn scale(self, k: f32) -> Self {
        Color {
            r: self.r * k,
            g: self.g * k,
            b: self.b * k,
            a: self.a * k,
        }
    }
    fn magnitude(self) -> f32 {
        (self.r * self.r + self.g * self.g + self.b * self.b + self.a * self.a).sqrt()
    }
    fn zero() -> Self {
        Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }
    }
    fn slot_mut(am: &mut AnimMap) -> &mut AnimMapTyped<Self> {
        &mut am.colors
    }
}
