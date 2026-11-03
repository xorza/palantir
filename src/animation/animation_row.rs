//! One animated value's cross-frame row: position, target and motion model.

use crate::animation::animated::Animated;
use crate::animation::animation_key::AnimationKey;
use crate::animation::animation_spec::AnimationMotion;
use crate::animation::duration;
use crate::animation::easing::Easing;
use crate::animation::spring::Spring;
use crate::primitives::math::animatable::Animatable;

/// State carried only by the active motion model; the variant is also the mode tag. Both models
/// hold an offset from the target in motion space, never the value: feeding a rounded value back
/// would stall a decay short of it, and a target that moves under one key carries the value with
/// it while the offset keeps its own clock.
///
/// A retarget follows the canonical models: a spring keeps its velocity (SwiftUI), a duration
/// turning back uses CSS Transitions' reversing-adjusted start and shortening. Refused: zeroing
/// an opposing velocity, a C¹ break; a full restart on reversal.
///
/// A run keeps the spec it started under until the next retarget, as CSS Transitions §2.3 keeps
/// a started transition's duration and timing function. Refused: taking a new spec mid-way from
/// the value shown, which stretched a run each frame the spec moved and broke C¹ on a change of
/// model with no change of target.
#[derive(Clone, Copy, Debug)]
pub(super) enum MotionRow<T: Animatable> {
    Duration {
        /// From the target to where the segment started, in motion space; it fades to zero
        /// along the ease.
        start_offset: T,
        elapsed: f32,
        secs: f32,
        ease: Easing,
        /// What a segment that turns back returns to, so a reversal is recognized after several.
        reversing_start: ReversingStart<T>,
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

/// CSS Transitions' reversing-adjusted start value. A row with no key compares values. A keyed
/// row compares keys, as its target moves with its inputs: the key it rested on, or `None` for a
/// segment that started mid-way, between two states.
#[derive(Clone, Copy, Debug)]
pub(super) enum ReversingStart<T> {
    Value(T),
    Key(Option<AnimationKey>),
}

/// A row taking a new target: where it is, what it headed for under which key, and what it heads
/// for now. A row that runs as a spring normalizes `cur` for spring arithmetic
/// ([`Animatable::normalize_for_spring`]), which the caller reads back.
#[derive(Debug)]
pub(super) struct Retarget<'a, T> {
    pub(super) cur: &'a mut T,
    pub(super) old_target: &'a T,
    pub(super) target: &'a T,
    pub(super) old_key: Option<AnimationKey>,
    pub(super) key: Option<AnimationKey>,
    /// Whether the row rested on `old_target`.
    pub(super) at_rest: bool,
}

impl<T: Animatable> MotionRow<T> {
    pub(super) fn new(motion: AnimationMotion, cur: &T, target: &T) -> Self {
        match motion {
            AnimationMotion::Duration { secs, ease } => Self::Duration {
                start_offset: offset(cur, target),
                elapsed: 0.0,
                secs,
                ease,
                reversing_start: ReversingStart::Value(cur.clone()),
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
            Self::Duration {
                start_offset,
                elapsed,
                secs,
                ease,
                shortening,
                ..
            } => {
                let progress = progress(*elapsed, *secs * *shortening);
                placed(
                    target,
                    &start_offset.clone().scale(1.0 - ease.apply(progress)),
                )
            }
            Self::Spring { offset, .. } => placed(target, offset),
        }
    }

    /// Restarts from `cur` toward `target` under `motion`, the spec in force at the change. A run
    /// takes a spec only here, as CSS Transitions §2.3 runs a started transition "based on the
    /// original timing function, duration, and delay" whatever they become, so a spec that
    /// changes mid-way neither jumps the value nor stretches the run.
    ///
    /// A spring keeps its velocity, so the value moves on without a kink (C¹), as SwiftUI's
    /// springs do. A duration that turns back to its reversing-adjusted start runs from `cur`
    /// over the share of its length the interrupted segment covered, by CSS Transitions'
    /// faster reversing, so a toggle undone mid-way takes as long as it ran, not the whole
    /// length again; the share is measured under the old length and applied to the new one, as
    /// CSS scales the new `transition-duration`. A change of model starts the new one from rest.
    pub(super) fn retarget(&mut self, change: &mut Retarget<'_, T>, motion: AnimationMotion) {
        match (self, motion) {
            (
                Self::Duration {
                    start_offset,
                    elapsed,
                    secs,
                    ease,
                    reversing_start,
                    shortening,
                },
                AnimationMotion::Duration {
                    secs: new_secs,
                    ease: new_ease,
                },
            ) => {
                let Retarget {
                    old_target,
                    target,
                    old_key,
                    key,
                    at_rest,
                    ..
                } = *change;
                let cur: &T = change.cur;
                let turns_back = match (key, &*reversing_start) {
                    (Some(key), ReversingStart::Key(from)) => *from == Some(key),
                    (None, ReversingStart::Value(from)) => from == target,
                    _ => false,
                };
                if turns_back {
                    let output = ease.apply(progress(*elapsed, *secs * *shortening));
                    *shortening = (output * *shortening + 1.0 - *shortening)
                        .abs()
                        .clamp(0.0, 1.0);
                } else {
                    *shortening = 1.0;
                }
                *reversing_start = match key {
                    Some(_) => {
                        ReversingStart::Key((turns_back || at_rest).then_some(old_key).flatten())
                    }
                    None if turns_back => ReversingStart::Value(old_target.clone()),
                    None => ReversingStart::Value(cur.clone()),
                };
                *start_offset = offset(cur, target);
                *elapsed = 0.0;
                *secs = new_secs;
                *ease = new_ease;
            }
            (
                Self::Spring {
                    velocity,
                    offset: at,
                    spring,
                },
                AnimationMotion::Spring(new_spring),
            ) => {
                change.cur.normalize_for_spring(change.target, velocity);
                *at = offset(change.cur, change.target);
                *spring = new_spring;
            }
            (this, motion) => {
                *this = Self::new(motion, change.cur, change.target);
                this.retarget(change, motion);
            }
        }
    }

    /// Whether `displacement` (and a spring's velocity) is too small to animate.
    pub(super) fn close_enough(&self, displacement: &T) -> bool {
        match self {
            Self::Duration { .. } => duration::within_snap_eps(displacement),
            Self::Spring {
                velocity, spring, ..
            } => spring.within_settle_eps(displacement, velocity),
        }
    }

    pub(super) fn stop(&mut self) {
        match self {
            Self::Duration { start_offset, .. } => *start_offset = T::zero(),
            Self::Spring {
                velocity, offset, ..
            } => {
                *velocity = T::zero();
                *offset = T::zero();
            }
        }
    }

    pub(super) fn advance(&mut self, target: T, dt: f32) -> Animated<T> {
        match self {
            Self::Duration {
                start_offset,
                elapsed,
                secs,
                ease,
                shortening,
                ..
            } => {
                *elapsed += dt;
                let progress = progress(*elapsed, *secs * *shortening);
                if progress >= 1.0 {
                    *start_offset = T::zero();
                    return Animated {
                        value: target,
                        settled: true,
                    };
                }
                Animated {
                    value: placed(&target, &start_offset.scale(1.0 - ease.apply(progress))),
                    settled: false,
                }
            }
            Self::Spring {
                velocity,
                offset,
                spring,
            } => {
                let step = spring.step(offset, velocity, dt);
                *velocity = step.velocity;
                *offset = step.offset;
                Animated {
                    value: if step.settled {
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

/// The offset from `target` to `cur`, in motion space.
pub(super) fn offset<T: Animatable>(cur: &T, target: &T) -> T {
    cur.to_motion().sub(&target.to_motion())
}

/// The value `offset` from `target`. An offset of no length is the target itself, exactly: the
/// round trip through motion space would change its low bits, and a settled row must rest on its
/// target. Its length, not `== zero()`: a derived offset keeps its snap and
/// lerp-only fields.
fn placed<T: Animatable>(target: &T, offset: &T) -> T {
    if offset.magnitude_squared() == 0.0 {
        return target.clone();
    }
    T::from_motion(target.to_motion().add(offset))
}

/// How far through a segment of `length` seconds `elapsed` is; a segment of no length is done.
#[inline]
fn progress(elapsed: f32, length: f32) -> f32 {
    if length > 0.0 { elapsed / length } else { 1.0 }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnimationRow<T: Animatable> {
    pub(super) target: T,
    /// The target's cause; see [`AnimationKey`].
    pub(super) key: Option<AnimationKey>,
    pub(super) motion: MotionRow<T>,
    /// Render-frame id of the last `tick`. A second `tick` in the same frame (multi-pass record)
    /// skips the advance but still retargets, and the end-of-frame sweep drops a row whose frame
    /// is not the one that just recorded.
    pub(super) ticked_at: u64,
    /// Cached settle state, letting `tick` skip the settle math on a steady
    /// row; the retarget compare still unfreezes it.
    pub(super) settled: bool,
}

impl<T: Animatable> AnimationRow<T> {
    pub(super) fn cur(&self) -> T {
        self.motion.cur(&self.target)
    }

    /// One `dt` toward the target from `cur`, its value now, unless pass A of this frame already
    /// spent it: pass B only updates the target, the offset or the velocity, and another `dt`
    /// would double the speed on any input frame.
    pub(super) fn advance_once(&mut self, cur: T, already_advanced: bool, dt: f32) -> Animated<T> {
        if already_advanced {
            return Animated {
                value: cur,
                settled: false,
            };
        }
        let step = self.motion.advance(self.target.clone(), dt);
        self.settled = step.settled;
        step
    }
}
