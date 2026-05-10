//! Critically-damped spring step (semi-implicit Euler), generic over
//! [`Animatable`]. One step advances `(current, velocity)` toward
//! `target` by `dt` seconds. Settle thresholds are tuned for
//! normalized 0..1 / pixel-scale values; visually settled animations
//! reach the threshold in <1s for typical stiffness.

use crate::animation::animatable::Animatable;

const POS_EPS: f32 = 0.001;
const VEL_EPS: f32 = 0.01;
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
    let displacement = current.sub(target);
    let spring_force = displacement.scale(-stiffness);
    let damp_force = velocity.scale(-damping);
    let accel = spring_force.add(damp_force);
    let new_velocity = velocity.add(accel.scale(dt));
    let new_current = current.add(new_velocity.scale(dt));
    let new_displacement = new_current.sub(target);
    if within_settle_eps(new_displacement, new_velocity) {
        SpringStep {
            current: target,
            velocity: T::zero(),
            settled: true,
        }
    } else {
        SpringStep {
            current: new_current,
            velocity: new_velocity,
            settled: false,
        }
    }
}
