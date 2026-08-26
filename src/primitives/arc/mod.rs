//! Circular-arc utilities for the native GPU stroke pipeline. Arcs
//! render exactly on the GPU (see `renderer::backend::curve_pipeline`);
//! what lives here is the CPU-side bbox that sizes the lowered record.

use crate::primitives::bezier::CurveBounds;
use glam::Vec2;
use std::f32::consts::FRAC_PI_2;

/// Tight axis-aligned bbox of the arc's centerline trace (no stroke
/// inflation). Angles follow the screen convention (0 = +x, y-down ⇒
/// increasing = clockwise); the sweep direction (`a0` vs `a1` order)
/// doesn't affect the bounds.
///
/// Extremes are the two endpoints plus one **exact** `center ± radius`
/// snap per quarter-axis the sweep crosses: `angle = k·π/2` points at
/// +x / +y / −x / −y for `k ≡ 0..3 (mod 4)`, so a crossing pins that
/// axis's bound directly — no trig in the loop, and only the first
/// four crossings matter (a full ±2π sweep covers every axis). Not
/// `const`: the endpoints need real trig, and `sin_cos` isn't
/// const-stable.
pub(crate) fn arc_bbox(center: Vec2, radius: f32, a0: f32, a1: f32) -> CurveBounds {
    let p_at = |a: f32| {
        let (s, c) = a.sin_cos();
        center + radius * Vec2::new(c, s)
    };
    let e0 = p_at(a0);
    let e1 = p_at(a1);
    // The AABB NaN contract, in its fixed-input form: a NaN centre,
    // radius, or angle propagates into `e0`/`e1` through `p_at`, so one
    // screen of the two endpoints covers every input, and `min`/`max`
    // below stay the plain laundering form.
    if e0.is_nan() || e1.is_nan() {
        return CurveBounds {
            lo: Vec2::NAN,
            hi: Vec2::NAN,
        };
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
    CurveBounds { lo, hi }
}

#[cfg(test)]
mod tests;
