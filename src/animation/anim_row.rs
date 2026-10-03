//! One animated value's cross-frame row: where it is, where it is going,
//! and the motion model carrying it there.

use crate::animation::anim_map_typed::TickResult;
use crate::animation::anim_spec::AnimMotion;
use crate::animation::animatable::Animatable;
use crate::animation::duration::within_duration_snap_eps;
use crate::animation::easing::Easing;
use crate::animation::spring::{self, within_settle_eps};

/// State carried only by the active motion model, with the parameters it
/// runs under. The variant is also the row's mode tag, so duration and
/// spring state cannot drift apart, and carrying the parameters is what
/// lets every step after [`Self::conform`] read the row alone.
///
/// **The value itself lives here too, once.** A duration holds it, since
/// its curve is sampled rather than accumulated. A spring holds only its
/// `offset` from the target and derives the value as `target + offset`:
/// feeding a rounded value back would lose every increment under half an
/// ulp of it — near 400 px, the last stretch of a decay moves less than
/// that per step and would stall short of the target for good — and a
/// second copy beside the offset would be a second truth to keep equal.
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
    /// `motion` at rest on `current`, heading for `target`.
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

    /// Where the value is, given the `target` it is heading for.
    pub(super) fn current(&self, target: &T) -> T {
        match self {
            Self::Duration { current, .. } => current.clone(),
            Self::Spring { offset, .. } => target.clone().add(offset.clone()),
        }
    }

    /// Run under `motion` from here on: the same kind keeps its state and
    /// takes the new parameters, another kind starts fresh from `current`.
    /// The one place the row's motion meets the caller's. Whether the kind
    /// changed.
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

    /// Start again from `current` toward `target`: a duration restarts its
    /// segment there; a spring measures its offset from it — the one place
    /// a spring rounds through the value — and keeps its velocity *only
    /// when it aids motion toward the new target*, preserving "fling
    /// through" continuations but killing reversal swings that would
    /// otherwise overshoot far past the new target (e.g. retargeting a
    /// toggle while the spring is mid-flight in the opposite direction).
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
                // Only the sign is read, so the velocity may be put in the
                // offset's unit first.
                let reach = spring::velocity_reach(velocity.clone(), *stiffness);
                if dot(reach, offset.clone()) > 0.0 {
                    *velocity = T::zero();
                }
            }
        }
    }

    /// Whether `displacement` from the target is too small to animate —
    /// with no velocity left either, for a spring.
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

    /// Forget any motion, at rest on `target`.
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

    /// Advance toward `target` by `dt` seconds.
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

/// Dot product via the polarization identity
/// `2·a·b = |a+b|² − |a|² − |b|²`, over the settle distance so each field
/// weighs in its own unit, and expressed in the existing `Animatable`
/// vocabulary so the trait need not widen. Used only on spring retarget to
/// decide whether residual velocity aids or opposes motion toward the new
/// target.
#[inline]
fn dot<T: Animatable>(a: T, b: T) -> f32 {
    // T is `Clone` (not `Copy`); each `Animatable` method consumes its
    // operand. Compute the magnitudes off the clones first, then let
    // `add` consume `a` and `b`.
    let mag_a = a.clone().settle_distance_squared();
    let mag_b = b.clone().settle_distance_squared();
    let sum = a.add(b).settle_distance_squared();
    0.5 * (sum - mag_a - mag_b)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnimRow<T: Animatable> {
    pub(super) target: T,
    pub(super) motion: MotionRow<T>,
    /// Set by every `tick`, cleared by `post_record`. Rows still
    /// `false` at `post_record` are dropped — that's how a slot whose
    /// caller stopped poking it (widget id stuck around but the
    /// animation site went away) gets evicted. Without this the
    /// `(WidgetId, AnimSlot)` map only shrinks on full widget removal.
    pub(super) touched: bool,
    /// `Ui` render-frame id at the last `tick` that ran the integrator
    /// step. A second `tick` in the same frame (multi-pass record:
    /// the frame driver re-runs `build` after an input action drains) sees
    /// this match and short-circuits the dt-driven advance, so the
    /// integrator advances exactly once per host frame. Retarget
    /// logic still runs in the short-circuited call so pass B's
    /// post-action target replaces pass A's stale one.
    pub(super) advanced_at: u64,
    /// Cached settle state, set true on insert / when the integrator
    /// or `within_settle_eps` confirms settlement, false on retarget.
    /// Lets `tick` fast-return on a steady-state row without the
    /// `sub` + `settle_distance_squared` settle math; the `PartialEq`
    /// retarget compare still runs so a target change unfreezes the
    /// row immediately.
    pub(super) settled: bool,
}

impl<T: Animatable> AnimRow<T> {
    /// Where the value is now.
    pub(super) fn current(&self) -> T {
        self.motion.current(&self.target)
    }
}
