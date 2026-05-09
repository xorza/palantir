//! Critically-damped spring step (semi-implicit Euler), generic over
//! [`Animatable`]. One step advances `(current, velocity)` toward
//! `target` by `dt` seconds. Settle thresholds are tuned for
//! normalized 0..1 / pixel-scale values; visually settled animations
//! reach the threshold in <1s for typical stiffness.

use crate::animation::animatable::Animatable;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub stiffness: f32,
    pub damping: f32,
}

pub(crate) const POS_EPS: f32 = 0.001;
pub(crate) const VEL_EPS: f32 = 0.01;

pub(crate) struct SpringStep<T: Animatable> {
    pub(crate) current: T,
    pub(crate) velocity: T,
    pub(crate) settled: bool,
}

impl Spring {
    pub(crate) fn step<T: Animatable>(
        self,
        current: T,
        velocity: T,
        target: T,
        dt: f32,
    ) -> SpringStep<T> {
        let displacement = current.sub(target);
        let spring_force = displacement.scale(-self.stiffness);
        let damp_force = velocity.scale(-self.damping);
        let accel = spring_force.add(damp_force);
        let new_velocity = velocity.add(accel.scale(dt));
        let new_current = current.add(new_velocity.scale(dt));
        let new_displacement = new_current.sub(target);
        let settled = new_displacement.magnitude() < POS_EPS && new_velocity.magnitude() < VEL_EPS;
        if settled {
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
}
