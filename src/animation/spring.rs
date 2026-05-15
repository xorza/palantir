//! Critically-damped spring step (semi-implicit Euler), generic over
//! [`Animatable`]. One step advances `(current, velocity)` toward
//! `target` by `dt` seconds. Settle thresholds are tuned for
//! normalized 0..1 / pixel-scale values; visually settled animations
//! reach the threshold in <1s for typical stiffness.

use crate::animation::animatable::Animatable;
use crate::ui::FIXED_STEP_DT;

// Settle tolerances. Bumped from 1e-3 / 1e-2 → 1e-2 / 1e-1 to give
// the integrator a more forgiving floor in pixel-scale animations
// where the f32 ULP near `cur ≈ 400` is already ~2.4e-5; sub-pixel
// drift below 0.01 px is visually indistinguishable and lets the
// spring settle a frame or two earlier on tight tolerances. The
// fixed-step accumulator on `Ui` is the primary fix for the
// NoVsync precision stall; this just trims residual settle time.
const POS_EPS: f32 = 0.01;
const VEL_EPS: f32 = 0.1;
const POS_EPS_SQ: f32 = POS_EPS * POS_EPS;
const VEL_EPS_SQ: f32 = VEL_EPS * VEL_EPS;

/// `(displacement, velocity)` is at the spring's settle floor — the
/// caller can snap to target and clear residual motion. Single source
/// of truth for the threshold; consumed both by [`step`] and by the
/// snap-if-close fast path in `AnimMapTyped::tick`.
#[inline]
pub(crate) fn within_settle_eps<T: Animatable>(displacement: T, velocity: T) -> bool {
    displacement.magnitude_squared() < POS_EPS_SQ && velocity.magnitude_squared() < VEL_EPS_SQ
}

pub(crate) struct SpringStep<T: Animatable> {
    pub(crate) current: T,
    pub(crate) velocity: T,
    pub(crate) settled: bool,
}

pub(crate) fn step<T: Animatable>(
    stiffness: f32,
    damping: f32,
    current: T,
    velocity: T,
    target: T,
    dt: f32,
) -> SpringStep<T> {
    // Sub-step so the inner Euler dt is always safely below the
    // stability boundary. `ceil(dt / FIXED_STEP_DT)` substeps of
    // equal width; for the typical 60Hz frame (dt = 0.016) this is 4
    // substeps, for the worst-case stalled frame (dt = MAX_DT = 0.1)
    // it's 24. Cheap relative to a single layout pass.
    let n = (dt / FIXED_STEP_DT).ceil().max(1.0);
    let sub_dt = dt / n;
    let mut cur = current;
    let mut vel = velocity;
    for _ in 0..(n as u32) {
        let displacement = cur.sub(target);
        let spring_force = displacement.scale(-stiffness);
        let damp_force = vel.scale(-damping);
        let accel = spring_force.add(damp_force);
        vel = vel.add(accel.scale(sub_dt));
        cur = cur.add(vel.scale(sub_dt));
    }
    // `Animatable::lerp(_, target, 0.0)` is the trick that pulls
    // `#[animate(snap)]` fields from `target` while leaving the
    // animated fields at their freshly-stepped value. Spring math
    // (sub/add/scale) passes snap fields through `self.field`, so
    // without this they'd ride the first-touch value frame after
    // frame and only catch up when `SpringStep` snaps to target on
    // settle — duration animations don't have this problem because
    // `lerp` is on the hot path there.
    let cur = T::lerp(cur, target, 0.0);
    let displacement = cur.sub(target);
    if within_settle_eps(displacement, vel) {
        SpringStep {
            current: target,
            velocity: T::zero(),
            settled: true,
        }
    } else {
        SpringStep {
            current: cur,
            velocity: vel,
            settled: false,
        }
    }
}
