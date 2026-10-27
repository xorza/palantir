//! One animated value's cross-frame row: position, target and motion model.

use crate::animation::animation_map_typed::TickResult;
use crate::animation::animation_spec::AnimationMotion;
use crate::animation::duration;
use crate::animation::easing::Easing;
use crate::animation::spring::Spring;
use crate::primitives::math::animatable::Animatable;

/// State carried only by the active motion model; the variant is also the mode
/// tag. A duration holds the value; a spring holds only its `offset` from the
/// target, since feeding a rounded value back would stall a decay short of it.
#[derive(Clone, Copy, Debug)]
pub(super) enum MotionRow<T: Animatable> {
    Duration {
        segment_start: T,
        cur: T,
        elapsed: f32,
        secs: f32,
        ease: Easing,
        /// CSS Transitions' reversing-adjusted start value: where a segment that turns back
        /// toward it returns to, so a reversal is recognized after several.
        reversing_start: T,
        /// The share of `secs` this segment runs for: 1, or less for a reversal, which covers
        /// only the way the interrupted segment came.
        shortening: f32,
    },
    Spring {
        /// In motion space, as `offset` is.
        velocity: T,
        offset: T,
        spring: Spring,
    },
}

impl<T: Animatable> MotionRow<T> {
    pub(super) fn new(motion: AnimationMotion, cur: &T, target: &T) -> Self {
        match motion {
            AnimationMotion::Duration { secs, ease } => Self::Duration {
                segment_start: cur.clone(),
                cur: cur.clone(),
                elapsed: 0.0,
                secs,
                ease,
                reversing_start: cur.clone(),
                shortening: 1.0,
            },
            AnimationMotion::Spring(spring) => Self::Spring {
                velocity: T::zero(),
                offset: offset(cur, target),
                spring,
            },
        }
    }

    pub(super) fn cur(&self, target: &T) -> T {
        match self {
            Self::Duration { cur, .. } => cur.clone(),
            Self::Spring { offset, .. } => placed(target, offset),
        }
    }

    /// Runs under `motion`: the same kind keeps its state, another starts fresh. Returns whether the kind changed.
    pub(super) fn conform(&mut self, motion: AnimationMotion, cur: &T, target: &T) -> bool {
        match (self, motion) {
            (
                Self::Duration { secs, ease, .. },
                AnimationMotion::Duration {
                    secs: new_secs,
                    ease: new_ease,
                },
            ) => {
                *secs = new_secs;
                *ease = new_ease;
                false
            }
            (Self::Spring { spring, .. }, AnimationMotion::Spring(new_spring)) => {
                *spring = new_spring;
                false
            }
            (this, motion) => {
                *this = Self::new(motion, cur, target);
                true
            }
        }
    }

    /// Restarts from `cur` toward `target`, away from `old_target`.
    ///
    /// A spring keeps its velocity, so the value moves on without a kink (C¹), as SwiftUI's
    /// springs do. A duration that turns back to its reversing-adjusted start runs from `cur`
    /// over the share of its length the interrupted segment covered, by CSS Transitions'
    /// faster reversing, so a toggle undone mid-way takes as long as it ran, not the whole
    /// length again.
    pub(super) fn retarget(&mut self, cur: &T, old_target: &T, target: &T) {
        match self {
            Self::Duration {
                segment_start,
                cur: at,
                elapsed,
                secs,
                ease,
                reversing_start,
                shortening,
            } => {
                if target == reversing_start {
                    let output = ease.apply(progress(*elapsed, *secs * *shortening));
                    *shortening = (output * *shortening + 1.0 - *shortening)
                        .abs()
                        .clamp(0.0, 1.0);
                    *reversing_start = old_target.clone();
                } else {
                    *shortening = 1.0;
                    *reversing_start = cur.clone();
                }
                *segment_start = cur.clone();
                *at = cur.clone();
                *elapsed = 0.0;
            }
            Self::Spring { offset: at, .. } => *at = offset(cur, target),
        }
    }

    /// Whether `displacement` (and a spring's velocity) is too small to animate.
    pub(super) fn close_enough(&self, displacement: T) -> bool {
        match self {
            Self::Duration { .. } => duration::within_snap_eps(displacement),
            Self::Spring {
                velocity, spring, ..
            } => spring.within_settle_eps(displacement, velocity.clone()),
        }
    }

    pub(super) fn stop(&mut self, target: &T) {
        match self {
            Self::Duration { cur, .. } => *cur = target.clone(),
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
                cur,
                elapsed,
                secs,
                ease,
                shortening,
                ..
            } => {
                *elapsed += dt;
                let progress = progress(*elapsed, *secs * *shortening);
                let settled = progress >= 1.0;
                *cur = if settled {
                    target
                } else {
                    T::lerp(segment_start.clone(), target, ease.apply(progress))
                };
                TickResult {
                    cur: cur.clone(),
                    settled,
                }
            }
            Self::Spring {
                velocity,
                offset,
                spring,
            } => {
                let step = spring.step(offset.clone(), velocity.clone(), dt);
                *velocity = step.velocity;
                *offset = step.offset;
                TickResult {
                    cur: if step.settled {
                        target
                    } else {
                        placed(&target, offset)
                    },
                    settled: step.settled,
                }
            }
        }
    }
}

/// The spring offset from `target` to `cur`, in motion space.
pub(super) fn offset<T: Animatable>(cur: &T, target: &T) -> T {
    cur.clone().to_motion().sub(target.clone().to_motion())
}

/// The value `offset` from `target`. An offset of no length is the target itself, exactly: the
/// round trip through motion space would change its low bits, and a settled row must rest on its
/// target. Its length, not `== zero()`: a derived offset keeps its snap fields.
fn placed<T: Animatable>(target: &T, offset: &T) -> T {
    if offset.clone().magnitude_squared() == 0.0 {
        return target.clone();
    }
    T::from_motion(target.clone().to_motion().add(offset.clone()))
}

/// How far through a segment of `length` seconds `elapsed` is; a segment of no length is done.
#[inline]
fn progress(elapsed: f32, length: f32) -> f32 {
    if length > 0.0 { elapsed / length } else { 1.0 }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnimationRow<T: Animatable> {
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

impl<T: Animatable> AnimationRow<T> {
    pub(super) fn cur(&self) -> T {
        self.motion.cur(&self.target)
    }
}
