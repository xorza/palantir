//! Critically-damped spring step (semi-implicit Euler). One step
//! advances `(current, velocity)` toward `target` by `dt` seconds.
//! Settle thresholds are tuned for normalized 0..1 values; pixel-scale
//! springs (typically 100s of px) settle visually well before the
//! threshold trips.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub stiffness: f32,
    pub damping: f32,
}

pub(crate) const POS_EPS: f32 = 0.001;
pub(crate) const VEL_EPS: f32 = 0.01;

pub(crate) struct SpringStep {
    pub(crate) current: f32,
    pub(crate) velocity: f32,
    pub(crate) settled: bool,
}

impl Spring {
    pub(crate) fn step(self, current: f32, velocity: f32, target: f32, dt: f32) -> SpringStep {
        let displacement = current - target;
        let accel = -self.stiffness * displacement - self.damping * velocity;
        let new_velocity = velocity + accel * dt;
        let new_current = current + new_velocity * dt;
        let new_displacement = new_current - target;
        let settled = new_displacement.abs() < POS_EPS && new_velocity.abs() < VEL_EPS;
        if settled {
            SpringStep {
                current: target,
                velocity: 0.0,
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
