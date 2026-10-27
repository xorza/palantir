//! The circular arc [`Shape::arc`](crate::widget::Shape::arc) strokes and the curve pipeline traces natively (see `gpu::curve_pipeline`), and its bbox.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::{self, vec2};
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, TAU};

/// A circular arc about `center`, owner-local, sweeping `sweep` radians from `start_angle`. Angles are screen-convention radians (0 = +x, increasing = clockwise); a negative sweep runs counter-clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircularArc {
    /// The circle's center.
    pub center: Vec2,
    /// The circle's radius.
    pub radius: f32,
    /// Where the arc starts, in radians.
    pub start_angle: f32,
    /// How far the arc turns from `start_angle`, in radians; `±2π` is a full circle, and [`Shape::arc`](crate::widget::Shape::arc) draws a longer sweep as one.
    pub sweep: f32,
}

impl CircularArc {
    /// Panics unless `center` is an [offset](crate::widget::domain::offset), `radius` a [length](crate::widget::domain::length) and both angles [angles](crate::widget::domain::angle).
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(self) {
        vec2::offset(self.center);
        domain::length(self.radius);
        domain::angle(self.start_angle);
        domain::angle(self.sweep);
    }

    /// The arc with `sweep` clamped to one full circle, `±2π`, its sign kept: a longer sweep traces the same circle.
    #[inline]
    pub(crate) const fn coerced(self) -> Self {
        Self {
            sweep: self.sweep.clamp(-TAU, TAU),
            ..self
        }
    }

    /// Where the arc ends: `start_angle + sweep`, the one place the sum is taken, so the bbox and the drawn arc agree.
    #[inline]
    pub(crate) const fn end_angle(self) -> f32 {
        self.start_angle + self.sweep
    }

    /// Tight bbox of the centerline (no stroke inflation); sweep order doesn't matter.
    ///
    /// Extremes are the endpoints plus an exact `center ± radius` per quarter-axis crossed. Not `const`: `sin_cos` isn't const-stable.
    pub(crate) fn bbox(self) -> Rect {
        let a1 = self.end_angle();
        let Self {
            center,
            radius,
            start_angle: a0,
            ..
        } = self;
        let p_at = |a: f32| {
            let (s, c) = a.sin_cos();
            center + radius * Vec2::new(c, s)
        };
        let e0 = p_at(a0);
        let e1 = p_at(a1);
        // A NaN center, radius or angle reaches `e0`/`e1` through `p_at`.
        if e0.is_nan() || e1.is_nan() {
            return Rect::NAN;
        }
        let mut lo = e0.min(e1);
        let mut hi = e0.max(e1);
        let (a_min, a_max) = if a0 <= a1 { (a0, a1) } else { (a1, a0) };
        // Reduced into one turn first: at a huge angle the quarter count saturates `i64`, and
        // `k0 + 3` overflows.
        let from = a_min.rem_euclid(TAU);
        let to = from + (a_max - a_min);
        let k0 = (from / FRAC_PI_2).ceil() as i64;
        let k1 = (to / FRAC_PI_2).floor() as i64;
        for k in k0..=k1.min(k0 + 3) {
            match k.rem_euclid(4) {
                0 => hi.x = center.x + radius,
                1 => hi.y = center.y + radius,
                2 => lo.x = center.x - radius,
                _ => lo.y = center.y - radius,
            }
        }
        Rect::from_min_max(lo, hi)
    }
}

#[cfg(test)]
mod tests;
