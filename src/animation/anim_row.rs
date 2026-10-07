//! One animated value's cross-frame row: position, target and motion model.

use crate::animation::anim_map_typed::TickResult;
use crate::animation::animatable::Animatable;
use crate::animation::animation_spec::AnimMotion;
use crate::animation::duration::within_duration_snap_eps;
use crate::animation::easing::Easing;
use crate::animation::spring::{self, within_settle_eps};

/// State carried only by the active motion model; the variant is also the mode
/// tag. A duration holds the value; a spring holds only its `offset` from the
/// target, since feeding a rounded value back would stall a decay short of it.
#[derive(Clone, Copy, Debug)]
pub(super) enum MotionRow<T: Animatable> {
    Duration {
        segment_start: T,
        current: T,
        elapsed: f32,
        secs: f32,
        ease: Easing,
    },
    Spring {
        velocity: T,
        offset: T,
        stiffness: f32,
        damping: f32,
    },
}

impl<T: Animatable> MotionRow<T> {
    pub(super) fn new(motion: AnimMotion, current: &T, target: &T) -> Self {
        match motion {
            AnimMotion::Duration { secs, ease } => Self::Duration {
                segment_start: current.clone(),
                current: current.clone(),
                elapsed: 0.0,
                secs,
                ease,
            },
            AnimMotion::Spring { stiffness, damping } => Self::Spring {
                velocity: T::zero(),
                offset: current.clone().sub(target.clone()),
                stiffness,
                damping,
            },
        }
    }

    pub(super) fn current(&self, target: &T) -> T {
        match self {
            Self::Duration { current, .. } => current.clone(),
            Self::Spring { offset, .. } => target.clone().add(offset.clone()),
        }
    }

    /// Runs under `motion`: the same kind keeps its state, another starts fresh. Returns whether the kind changed.
    pub(super) fn conform(&mut self, motion: AnimMotion, current: &T, target: &T) -> bool {
        match (self, motion) {
            (
                Self::Duration { secs, ease, .. },
                AnimMotion::Duration {
                    secs: new_secs,
                    ease: new_ease,
                },
            ) => {
                *secs = new_secs;
                *ease = new_ease;
                false
            }
            (
                Self::Spring {
                    stiffness, damping, ..
                },
                AnimMotion::Spring {
                    stiffness: new_stiffness,
                    damping: new_damping,
                },
            ) => {
                *stiffness = new_stiffness;
                *damping = new_damping;
                false
            }
            (this, motion) => {
                *this = Self::new(motion, current, target);
                true
            }
        }
    }

    /// Restarts from `current` toward `target`. A spring keeps its velocity
    /// only when it aids motion toward the new target, so a reversal doesn't overshoot.
    pub(super) fn retarget(&mut self, current: &T, target: &T) {
        match self {
            Self::Duration {
                segment_start,
                current: at,
                elapsed,
                ..
            } => {
                *segment_start = current.clone();
                *at = current.clone();
                *elapsed = 0.0;
            }
            Self::Spring {
                velocity,
                offset,
                stiffness,
                ..
            } => {
                *offset = current.clone().sub(target.clone());
                let reach = spring::velocity_reach(velocity.clone(), *stiffness);
                if dot(reach, offset.clone()) > 0.0 {
                    *velocity = T::zero();
                }
            }
        }
    }

    /// Whether `displacement` (and a spring's velocity) is too small to animate.
    pub(super) fn close_enough(&self, displacement: T) -> bool {
        match self {
            Self::Duration { .. } => within_duration_snap_eps(displacement),
            Self::Spring {
                velocity,
                stiffness,
                ..
            } => within_settle_eps(displacement, velocity.clone(), *stiffness),
        }
    }

    pub(super) fn stop(&mut self, target: &T) {
        match self {
            Self::Duration { current, .. } => *current = target.clone(),
            Self::Spring {
                velocity, offset, ..
            } => {
                *velocity = T::zero();
                *offset = T::zero();
            }
        }
    }

    pub(super) fn advance(&mut self, target: T, dt: f32) -> TickResult<T> {
        match self {
            Self::Duration {
                segment_start,
                current,
                elapsed,
                secs,
                ease,
            } => {
                *elapsed += dt;
                let progress = *elapsed / *secs;
                let settled = progress >= 1.0;
                *current = if settled {
                    target
                } else {
                    T::lerp(segment_start.clone(), target, ease.apply(progress))
                };
                TickResult {
                    current: current.clone(),
                    settled,
                }
            }
            Self::Spring {
                velocity,
                offset,
                stiffness,
                damping,
            } => {
                let step = spring::step(*stiffness, *damping, offset.clone(), velocity.clone(), dt);
                *velocity = step.velocity;
                *offset = step.offset;
                TickResult {
                    current: if step.settled {
                        target
                    } else {
                        target.add(offset.clone())
                    },
                    settled: step.settled,
                }
            }
        }
    }
}

/// Dot product via the polarization identity `2·a·b = |a+b|² − |a|² − |b|²`,
/// used on spring retarget to tell whether residual velocity aids the new target.
#[inline]
fn dot<T: Animatable>(a: T, b: T) -> f32 {
    let mag_a = a.clone().settle_distance_squared();
    let mag_b = b.clone().settle_distance_squared();
    let sum = a.add(b).settle_distance_squared();
    0.5 * (sum - mag_a - mag_b)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnimRow<T: Animatable> {
    pub(super) target: T,
    pub(super) motion: MotionRow<T>,
    /// Set by every `tick`, cleared by `post_record`; rows still `false` then are evicted.
    pub(super) touched: bool,
    /// Render-frame id of the last integrator step. A second `tick` in the same
    /// frame (multi-pass record) skips the advance but still retargets.
    pub(super) advanced_at: u64,
    /// Cached settle state, letting `tick` skip the settle math on a steady
    /// row; the retarget compare still unfreezes it.
    pub(super) settled: bool,
}

impl<T: Animatable> AnimRow<T> {
    pub(super) fn current(&self) -> T {
        self.motion.current(&self.target)
    }
}
