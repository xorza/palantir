//! The curves the crate ships, as plain `fn`s a
//! [`PaintAnim`](crate::PaintAnim) can take.
//!
//! Each maps a phase in `[0, 1)` to a unit value in `[0, 1]`. A caller's
//! own curve is any function with that shape — there is nothing to
//! implement and nothing to register.

use std::f32::consts::TAU;

/// The phase itself. A ramp from the range's start to its end, and what
/// a spinner turns on.
#[inline]
pub fn linear(t: f32) -> f32 {
    t
}

/// One for the first half of the period, zero for the second — the caret
/// blink.
///
/// Pair it with [`PaintAnim::steps(2)`](crate::PaintAnim::steps): the
/// value changes twice a period, so a frame in between buys an identical
/// picture.
#[inline]
pub fn square(t: f32) -> f32 {
    if t < 0.5 { 1.0 } else { 0.0 }
}

/// Zero at both ends and one in the middle, on a raised cosine. Breathing
/// rather than sawing: the value arrives at each end with zero slope, so
/// a repeating pass has no visible seam.
#[inline]
pub fn sine(t: f32) -> f32 {
    (1.0 - (t * TAU).cos()) * 0.5
}
