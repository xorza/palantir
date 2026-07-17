//! Damped spring step (semi-implicit Euler), generic over [`Animatable`].
//! Accepted parameters have a bounded convergence rate and integration cost;
//! each call adapts its substep to the spring's actual stability boundary.

use crate::animation::animatable::Animatable;
use crate::common::time::{ANIM_SUBSTEP_DT, MAX_ANIM_DT};
use crate::primitives::approx::EPS;

const STABILITY_SAFETY: f64 = 0.8;
const MIN_DECAY_RATE: f64 = 1.0;
const MAX_SUBSTEPS_PER_FRAME: f32 = 256.0;

// Spring settle tolerances. Bumped from 1e-3 / 1e-2 → 1e-2 / 1e-1 to
// give the integrator a more forgiving floor in pixel-scale animations
// where the f32 ULP near `cur ≈ 400` is already ~2.4e-5; sub-pixel
// drift below 0.01 px is visually indistinguishable and lets the
// spring settle a frame or two earlier on tight tolerances. The
// fixed-step accumulator on `Ui` is the primary fix for the
// NoVsync precision stall; this just trims residual settle time.
//
// These are intentionally *loose* — a spring's job is to converge, and
// the eye can't see the last 0.01 of travel. The duration path uses a
// far tighter floor (`DURATION_SNAP_EPS`); see `within_duration_snap_eps`.
const POS_EPS: f32 = 0.01;
const VEL_EPS: f32 = 0.1;
const POS_EPS_SQ: f32 = POS_EPS * POS_EPS;
const VEL_EPS_SQ: f32 = VEL_EPS * VEL_EPS;

// Duration snap-if-close floor. Far tighter than the spring floor: a
// duration animation should run its full designed curve for *any*
// visible target change, and snap-without-animating only when the
// target moved by sub-perceptual drift (ulp rounding in upstream theme
// math). The spring floor is pixel-scale-loose; reusing it here made
// sub-1% colour transitions (0..1 linear-RGB) snap instead of ease.
// `EPS = 1e-4` is below 8-bit colour precision and sub-pixel position
// resolution, so a target delta under it is genuinely invisible.
// Duration rows carry no velocity, so this is a position-only check;
// curve completion is handled by the `t >= 1.0` arm in `tick`, not here.
const DURATION_SNAP_EPS_SQ: f32 = EPS * EPS;

/// `(displacement, velocity)` is at the spring's settle floor — the
/// caller can snap to target and clear residual motion. Single source
/// of truth for the threshold; consumed both by [`step`] and by the
/// spring arm of the snap-if-close fast path in `AnimMapTyped::tick`.
#[inline]
pub(crate) fn within_settle_eps<T: Animatable>(displacement: T, velocity: T) -> bool {
    displacement.magnitude_squared() < POS_EPS_SQ && velocity.magnitude_squared() < VEL_EPS_SQ
}

/// `displacement` is below the duration snap floor — the caller can
/// snap to target without animating, because the target barely moved.
/// Position-only (duration rows have no velocity). Consumed by the
/// duration arm of the snap-if-close fast path in `AnimMapTyped::tick`.
#[inline]
pub(crate) fn within_duration_snap_eps<T: Animatable>(displacement: T) -> bool {
    displacement.magnitude_squared() < DURATION_SNAP_EPS_SQ
}

pub(crate) struct SpringStep<T: Animatable> {
    pub(crate) current: T,
    pub(crate) velocity: T,
    pub(crate) settled: bool,
}

pub(crate) fn stable_substep_dt(stiffness: f32, damping: f32) -> f32 {
    let stiffness = f64::from(stiffness);
    let damping = f64::from(damping);
    let boundary = 4.0 / ((damping * damping + 4.0 * stiffness).sqrt() + damping);
    ANIM_SUBSTEP_DT.min((boundary * STABILITY_SAFETY) as f32)
}

fn decay_rate(stiffness: f32, damping: f32) -> f64 {
    let stiffness = f64::from(stiffness);
    let half_damping = f64::from(damping) * 0.5;
    let discriminant = half_damping * half_damping - stiffness;
    if discriminant <= 0.0 {
        half_damping
    } else {
        stiffness / (half_damping + discriminant.sqrt())
    }
}

pub(crate) fn params_are_valid(stiffness: f32, damping: f32, substep_dt: f32) -> bool {
    if !(stiffness.is_finite() && stiffness > 0.0 && damping.is_finite() && damping > 0.0) {
        return false;
    }
    decay_rate(stiffness, damping) >= MIN_DECAY_RATE
        && substep_dt > 0.0
        && (MAX_ANIM_DT / substep_dt).ceil() <= MAX_SUBSTEPS_PER_FRAME
}

pub(crate) fn step<T: Animatable>(
    stiffness: f32,
    damping: f32,
    substep_dt: f32,
    current: T,
    velocity: T,
    target: T,
    dt: f32,
) -> SpringStep<T> {
    debug_assert!(dt.is_finite() && (0.0..=MAX_ANIM_DT).contains(&dt));
    let n = (dt / substep_dt).ceil().max(1.0);
    let sub_dt = dt / n;
    let mut cur = current;
    let mut vel = velocity;
    // `T: Animatable` is now `Clone` (not `Copy`) so heavyweights like
    // `Background` only `Copy` when their fields actually are. For the
    // common scalar/vector animations these clones compile to noops;
    // for the few wide types they're explicit by design.
    for _ in 0..(n as u32) {
        let displacement = cur.clone().sub(target.clone());
        let spring_force = displacement.scale(-stiffness);
        let damp_force = vel.clone().scale(-damping);
        let accel = spring_force.add(damp_force);
        vel = vel.add(accel.scale(sub_dt));
        cur = cur.add(vel.clone().scale(sub_dt));
    }
    // `Animatable::lerp(_, target, 0.0)` is the trick that pulls
    // `#[animate(snap)]` fields from `target` while leaving the
    // animated fields at their freshly-stepped value. Spring math
    // (sub/add/scale) passes snap fields through `self.field`, so
    // without this they'd ride the first-touch value frame after
    // frame and only catch up when `SpringStep` snaps to target on
    // settle — duration animations don't have this problem because
    // `lerp` is on the hot path there.
    let cur = T::lerp(cur, target.clone(), 0.0);
    let displacement = cur.clone().sub(target.clone());
    if within_settle_eps(displacement, vel.clone()) {
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
