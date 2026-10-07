//! Circular-arc bbox for the native GPU stroke pipeline (see `gpu::curve_pipeline`).

use crate::primitives::geometry::rect::Rect;
use glam::Vec2;
use std::f32::consts::FRAC_PI_2;

/// Tight bbox of the arc's centerline (no stroke inflation). Angles are screen-convention (0 = +x, increasing = clockwise); sweep order doesn't matter.
///
/// Extremes are the endpoints plus an exact `center ± radius` per quarter-axis crossed. Not `const`: `sin_cos` isn't const-stable.
pub(crate) fn bbox(center: Vec2, radius: f32, a0: f32, a1: f32) -> Rect {
    let p_at = |a: f32| {
        let (s, c) = a.sin_cos();
        center + radius * Vec2::new(c, s)
    };
    let e0 = p_at(a0);
    let e1 = p_at(a1);
    // A NaN centre, radius or angle reaches `e0`/`e1` through `p_at`.
    if e0.is_nan() || e1.is_nan() {
        return Rect::NAN;
    }
    let mut lo = e0.min(e1);
    let mut hi = e0.max(e1);
    let (a_min, a_max) = if a0 <= a1 { (a0, a1) } else { (a1, a0) };
    let k0 = (a_min / FRAC_PI_2).ceil() as i64;
    let k1 = (a_max / FRAC_PI_2).floor() as i64;
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

#[cfg(test)]
mod tests;
