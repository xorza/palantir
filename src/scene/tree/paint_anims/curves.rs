//! The curves the crate ships, as plain `fn`s a [`PaintAnimation`](crate::widget::PaintAnimation) takes: a phase in `[0, 1)` to a value in `[0, 1]`.

use std::f32::consts::TAU;

/// The phase itself: a ramp.
#[inline]
pub const fn linear(t: f32) -> f32 {
    t
}

/// One for the first half of the period, zero for the second (the caret blink); pair with [`PaintAnimation::with_steps(2)`](crate::widget::PaintAnimation::with_steps).
#[inline]
pub const fn square(t: f32) -> f32 {
    if t < 0.5 { 1.0 } else { 0.0 }
}

/// A raised cosine, zero at both ends; the zero slope leaves no seam when repeating.
#[inline]
pub fn sine(t: f32) -> f32 {
    (1.0 - (t * TAU).cos()) * 0.5
}
