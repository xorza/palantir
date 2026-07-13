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
mod tests {
    use crate::primitives::arc::arc_bbox;
    use glam::Vec2;
    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    const C: Vec2 = Vec2::new(10.0, 20.0);
    const R: f32 = 5.0;

    fn assert_bounds(a0: f32, a1: f32, lo: Vec2, hi: Vec2) {
        let b = arc_bbox(C, R, a0, a1);
        assert!(
            (b.lo - lo).length() < 1e-4 && (b.hi - hi).length() < 1e-4,
            "arc [{a0}, {a1}]: got lo {:?} hi {:?}, want lo {lo:?} hi {hi:?}",
            b.lo,
            b.hi,
        );
    }

    /// Hand-computed bounds per sweep case. Screen convention: angle 0
    /// = +x, π/2 = +y (down). E.g. the quarter arc [0, π/2] traces
    /// from (C.x + R, C.y) to (C.x, C.y + R), staying in the +x/+y
    /// quadrant — bbox spans exactly those two endpoints.
    #[test]
    fn quarter_half_and_full_sweeps() {
        // Quarter [0, π/2]: endpoints only, no axis crossing inside.
        assert_bounds(
            0.0,
            FRAC_PI_2,
            Vec2::new(C.x, C.y),
            Vec2::new(C.x + R, C.y + R),
        );
        // Half [0, π]: crosses +y at π/2 → bbox reaches C.y + R.
        assert_bounds(
            0.0,
            PI,
            Vec2::new(C.x - R, C.y),
            Vec2::new(C.x + R, C.y + R),
        );
        // Full circle: center ± radius on both axes.
        assert_bounds(0.0, TAU, C - Vec2::splat(R), C + Vec2::splat(R));
        // 3/4 sweep [0, 3π/2] (the spinner's arc): crosses +y and -x,
        // misses only the top-right-of-(-y) quadrant gap; +x endpoint
        // caps the right edge.
        assert_bounds(
            0.0,
            1.5 * PI,
            C - Vec2::splat(R),
            Vec2::new(C.x + R, C.y + R),
        );
    }

    /// Negative sweep covers the same trace as its reversed positive
    /// twin, and off-origin angle windows pick interior extremes.
    #[test]
    fn negative_sweep_and_offset_window() {
        // [π/2, -π/2] (negative direction) == [-π/2, π/2]: crosses +x.
        let fwd = arc_bbox(C, R, -FRAC_PI_2, FRAC_PI_2);
        let rev = arc_bbox(C, R, FRAC_PI_2, -FRAC_PI_2);
        assert!((fwd.lo - rev.lo).length() < 1e-6);
        assert!((fwd.hi - rev.hi).length() < 1e-6);
        assert_bounds(
            -FRAC_PI_2,
            FRAC_PI_2,
            Vec2::new(C.x, C.y - R),
            Vec2::new(C.x + R, C.y + R),
        );
        // Window far from 0: [2π + π/4, 2π + 3π/4] crosses +y at
        // 2π + π/2. Endpoints sit at ±R·cos(π/4) in x, +R·sin(π/4) in y.
        let cos45 = 0.5f32.sqrt();
        assert_bounds(
            TAU + 0.25 * PI,
            TAU + 0.75 * PI,
            Vec2::new(C.x - R * cos45, C.y + R * cos45),
            Vec2::new(C.x + R * cos45, C.y + R),
        );
    }
}
