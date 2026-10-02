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
#[derive(Clone, Copy, Debug)]
pub(super) enum MotionRow<T: Animatable> {
    Duration {
        segment_start: T,
        elapsed: f32,
        secs: f32,
        ease: Easing,
    },
    Spring {
        velocity: T,
        stiffness: f32,
        damping: f32,
    },
}

impl<T: Animatable> MotionRow<T> {
    pub(super) fn new(motion: AnimMotion, current: &T) -> Self {
        match motion {
            AnimMotion::Duration { secs, ease } => Self::Duration {
                segment_start: current.clone(),
                elapsed: 0.0,
                secs,
                ease,
            },
            AnimMotion::Spring { stiffness, damping } => Self::Spring {
                velocity: T::zero(),
                stiffness,
                damping,
            },
        }
    }

    /// Run under `motion` from here on: the same kind keeps its state and
    /// takes the new parameters, another kind starts fresh from `current`.
    /// The one place the row's motion meets the caller's.
    pub(super) fn conform(&mut self, motion: AnimMotion, current: &T) {
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
            }
            (this, motion) => *this = Self::new(motion, current),
        }
    }

    /// The target moved: a duration restarts its segment from `current`;
    /// a spring keeps its velocity *only when it aids motion toward the
    /// new target* — preserving "fling through" continuations but killing
    /// reversal swings that would otherwise overshoot far past the new
    /// target (e.g. retargeting a toggle while the spring is mid-flight in
    /// the opposite direction).
    pub(super) fn retarget(&mut self, current: &T, target: &T) {
        match self {
            Self::Duration {
                segment_start,
                elapsed,
                ..
            } => {
                *segment_start = current.clone();
                *elapsed = 0.0;
            }
            Self::Spring { velocity, .. } => {
                let to_target = target.clone().sub(current.clone());
                if dot(velocity.clone(), to_target) < 0.0 {
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
            Self::Spring { velocity, .. } => within_settle_eps(displacement, velocity.clone()),
        }
    }

    /// Forget any motion, at rest on the target.
    pub(super) fn stop(&mut self) {
        if let Self::Spring { velocity, .. } = self {
            *velocity = T::zero();
        }
    }

    /// Advance `current` toward `target` by `dt` seconds.
    pub(super) fn advance(&mut self, current: T, target: T, dt: f32) -> TickResult<T> {
        match self {
            Self::Duration {
                segment_start,
                elapsed,
                secs,
                ease,
            } => {
                *elapsed += dt;
                let progress = *elapsed / *secs;
                if progress >= 1.0 {
                    TickResult {
                        current: target,
                        settled: true,
                    }
                } else {
                    TickResult {
                        current: T::lerp(segment_start.clone(), target, ease.apply(progress)),
                        settled: false,
                    }
                }
            }
            Self::Spring {
                velocity,
                stiffness,
                damping,
            } => {
                let step =
                    spring::step(*stiffness, *damping, current, velocity.clone(), target, dt);
                *velocity = step.velocity;
                TickResult {
                    current: step.current,
                    settled: step.settled,
                }
            }
        }
    }
}

/// Dot product via the polarization identity
/// `2·a·b = |a+b|² − |a|² − |b|²`, expressed in the existing
/// `Animatable` vocabulary (add + magnitude_squared) so we don't have
/// to widen the trait. Used only on spring retarget to decide whether
/// residual velocity aids or opposes motion toward the new target.
#[inline]
fn dot<T: Animatable>(a: T, b: T) -> f32 {
    // T is `Clone` (not `Copy`); each `Animatable` method consumes its
    // operand. Compute the magnitudes off the clones first, then let
    // `add` consume `a` and `b`.
    let mag_a = a.clone().magnitude_squared();
    let mag_b = b.clone().magnitude_squared();
    let sum = a.add(b).magnitude_squared();
    0.5 * (sum - mag_a - mag_b)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnimRow<T: Animatable> {
    pub(super) current: T,
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
    /// `sub` + `magnitude_squared` settle math; the `PartialEq`
    /// retarget compare still runs so a target change unfreezes the
    /// row immediately.
    pub(super) settled: bool,
}
