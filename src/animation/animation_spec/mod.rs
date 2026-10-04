//! The authored animation spec: which motion model a value travels
//! under, and the parameters that model was authored with.

use crate::animation::duration::{DURATION_ERROR, MAX_DURATION, duration_is_valid};
use crate::animation::easing::Easing;
use crate::animation::spring::{SPRING_ERROR, params_are_valid as spring_params_are_valid};
use crate::primitives::math::domain::EPS;
use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Duration;

/// How a value moves toward its target. Animation itself is opt-in
/// at the call site — pass `None` to [`crate::Ui::animate`] (or omit
/// the field on a theme) when you want snap-to-target behavior.
/// `AnimationSpec` only describes what motion looks like *when there is
/// motion*; "no animation" lives in `Option<AnimationSpec>`, not as a
/// variant here.
///
/// Wire format is internally tagged on `kind` (snake_case), so theme
/// files read cleanly:
///
/// ```toml
/// [theme.button.defaults.animation]
/// kind = "duration"
/// secs = 0.12
/// ease = "out_cubic"
///
/// [theme.button.defaults.animation]
/// kind = "spring"
/// stiffness = 170.0
/// damping = 26.0
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationSpec {
    pub(super) motion: AnimMotion,
}

/// The motion model a spec was authored under, plus its
/// parameters. Kept private to the module: the public surface is
/// [`AnimationSpec`]'s constructors, and every reader is an animation-row
/// step that matches on it.
///
/// Also the wire shape — every field here is authored, so
/// [`AnimationSpec`]'s hand-written impls delegate to this one and spend
/// themselves on validation alone.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum AnimMotion {
    Duration { secs: f32, ease: Easing },
    Spring { stiffness: f32, damping: f32 },
}

impl AnimationSpec {
    /// 120 ms ease-out-cubic. Snappy hover/press default.
    pub const FAST: Self = Self {
        motion: AnimMotion::Duration {
            secs: 0.12,
            ease: Easing::OutCubic,
        },
    };
    /// 200 ms ease-out-cubic. Popup reveal / panel slide default.
    pub const MEDIUM: Self = Self {
        motion: AnimMotion::Duration {
            secs: 0.2,
            ease: Easing::OutCubic,
        },
    };
    /// No motion: the value lands on its target in one frame.
    ///
    /// The named form of the "do not animate" case, so a call site says
    /// which it means instead of leaving a bare `None` to be read. It is
    /// what [`Ui::animate`](crate::Ui::animate) does for `None` too — a
    /// zero-second duration is [`Self::is_instant`], and both short-circuit
    /// on that.
    pub const SNAP: Self = Self::duration_from_validated(0.0, Easing::Linear);
    /// Near-critically-damped spring tuned as a general-purpose default.
    pub const SPRING: Self = Self {
        motion: AnimMotion::Spring {
            stiffness: 170.0,
            damping: 26.0,
        },
    };

    /// Construct a duration animation that runs for `length`. A length
    /// under `1e-4` seconds canonicalizes to an instant snap.
    ///
    /// # Panics
    ///
    /// Panics when `length` is longer than 60 seconds.
    #[track_caller]
    pub const fn duration(length: Duration, ease: Easing) -> Self {
        assert!(
            length.as_nanos() <= MAX_DURATION.as_nanos(),
            "{}",
            DURATION_ERROR
        );
        Self::duration_from_validated(length.as_secs_f32(), ease)
    }

    const fn duration_from_validated(secs: f32, ease: Easing) -> Self {
        let secs = if secs < EPS { 0.0 } else { secs };
        Self {
            motion: AnimMotion::Duration { secs, ease },
        }
    }

    /// Construct a damped spring whose convergence rate stays within the
    /// supported UI-animation domain.
    ///
    /// The step is a closed-form transition, so stiffness costs nothing
    /// and carries no stability bound. What is still checked is that the
    /// spring *arrives*, decaying at 1/s or faster, and that a 60 Hz
    /// display can show it: an underdamped spring must swing slower than
    /// 30 Hz, `√(stiffness − damping²/4) < 60π` rad/s. A faster swing lands
    /// each frame on an arbitrary phase.
    ///
    /// # Panics
    ///
    /// Panics when either parameter is non-positive or non-finite, when
    /// the slowest decay rate is below 1/s, or when the spring swings at
    /// 30 Hz or faster. Raise `damping` or lower `stiffness` for the last.
    #[track_caller]
    pub fn spring(stiffness: f32, damping: f32) -> Self {
        assert!(
            spring_params_are_valid(stiffness, damping),
            "{SPRING_ERROR}"
        );
        Self {
            motion: AnimMotion::Spring { stiffness, damping },
        }
    }

    /// True when this spec collapses to a single-frame snap — a
    /// `Duration` canonicalized to zero seconds. Springs are never instant by
    /// construction. `Ui::animate` short-circuits on this and on `None`.
    #[inline(always)]
    pub const fn is_instant(self) -> bool {
        match self.motion {
            AnimMotion::Duration { secs, .. } => secs == 0.0,
            AnimMotion::Spring { .. } => false,
        }
    }
}

impl Serialize for AnimationSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.motion.serialize(serializer)
    }
}

/// Validating, so a hand-written impl rather than `#[serde(transparent)]`:
/// a theme file is untrusted input, and the bounds [`AnimationSpec::duration`]
/// and [`AnimationSpec::spring`] assert on have to hold for a spec that arrived
/// over the wire too. Bad data is an `Err` here rather than the panic those
/// two raise.
impl<'de> Deserialize<'de> for AnimationSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match AnimMotion::deserialize(deserializer)? {
            AnimMotion::Duration { secs, ease } => {
                if !duration_is_valid(secs) {
                    return Err(D::Error::custom(DURATION_ERROR));
                }
                Ok(Self::duration_from_validated(secs, ease))
            }
            AnimMotion::Spring { stiffness, damping } => {
                if !spring_params_are_valid(stiffness, damping) {
                    return Err(D::Error::custom(SPRING_ERROR));
                }
                Ok(Self {
                    motion: AnimMotion::Spring { stiffness, damping },
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
