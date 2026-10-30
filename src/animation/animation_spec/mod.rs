//! The authored animation spec: which motion model a value travels under, and
//! its parameters.

use crate::animation::duration::{self, DURATION_ERROR, MAX_DURATION, MIN_DURATION_SECS};
use crate::animation::easing::Easing;
use crate::animation::spring::{SPRING_ERROR, Spring};
use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Duration;

/// How a value moves toward its target. Pass `None` to [`crate::Ui::animate`],
/// or leave a theme slot's `animation` unset, to snap; the stock buttons,
/// menu-bar titles, menu rows and tab chips take [`Metrics::transition`](crate::Metrics::transition),
/// [`Self::FAST`] by default. `AnimationSpec` only describes
/// motion *when there is some*.
///
/// Wire format is internally tagged on `kind` (snake_case):
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
    pub(super) motion: AnimationMotion,
}

/// The motion model and parameters a spec was authored under. Module-private;
/// also the wire shape, so [`AnimationSpec`]'s hand-written impls only add
/// validation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum AnimationMotion {
    Duration { secs: f32, ease: Easing },
    Spring(Spring),
}

impl AnimationSpec {
    /// 120 ms ease-out-cubic. Snappy hover/press default.
    pub const FAST: Self = Self {
        motion: AnimationMotion::Duration {
            secs: 0.12,
            ease: Easing::OutCubic,
        },
    };
    /// 200 ms ease-out-cubic. Popup reveal / panel slide default.
    pub const MEDIUM: Self = Self {
        motion: AnimationMotion::Duration {
            secs: 0.2,
            ease: Easing::OutCubic,
        },
    };
    /// No motion: the value lands on its target in one frame. The named form of
    /// "do not animate"; `None` in [`Ui::animate`](crate::Ui::animate) does the same,
    /// and both short-circuit on [`Self::is_instant`].
    pub const SNAP: Self = Self::duration_from_validated(0.0, Easing::Linear);
    /// Near-critically-damped spring tuned as a general-purpose default.
    pub const SPRING: Self = Self {
        motion: AnimationMotion::Spring(Spring::DEFAULT),
    };

    /// Construct a duration animation of `length`; under 0.1 ms it canonicalizes to an
    /// instant snap, as no frame could show its motion.
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
        let secs = if secs < MIN_DURATION_SECS { 0.0 } else { secs };
        Self {
            motion: AnimationMotion::Duration { secs, ease },
        }
    }

    /// Construct a damped spring whose convergence stays within the supported
    /// UI-animation domain.
    ///
    /// The step is closed-form, so stiffness carries no stability bound. Checked:
    /// the spring *arrives* (decay of 1/s or faster) and a 60 Hz display can show
    /// it (underdamped swing under 30 Hz, `sqrt(stiffness - damping^2/4) < 60*pi`
    /// rad/s).
    ///
    /// # Panics
    ///
    /// Panics when either parameter is non-positive or non-finite, when
    /// the slowest decay rate is below 1/s, or when the spring swings at
    /// 30 Hz or faster. Raise `damping` or lower `stiffness` for the last.
    #[track_caller]
    pub fn spring(stiffness: f32, damping: f32) -> Self {
        let spring = Spring { stiffness, damping };
        assert!(spring.is_valid(), "{SPRING_ERROR}");
        Self {
            motion: AnimationMotion::Spring(spring),
        }
    }

    /// True when this spec is a single-frame snap (a `Duration` canonicalized to
    /// zero); springs never are. `Ui::animate` short-circuits on this and `None`.
    #[inline(always)]
    pub const fn is_instant(self) -> bool {
        match self.motion {
            AnimationMotion::Duration { secs, .. } => secs == 0.0,
            AnimationMotion::Spring(_) => false,
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

/// Hand-written (not `#[serde(transparent)]`) because a theme file is untrusted:
/// the bounds [`AnimationSpec::duration`] and [`AnimationSpec::spring`] assert
/// must hold over the wire too, as an `Err` instead of a panic.
impl<'de> Deserialize<'de> for AnimationSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match AnimationMotion::deserialize(deserializer)? {
            AnimationMotion::Duration { secs, ease } => {
                if !duration::is_valid(secs) {
                    return Err(D::Error::custom(DURATION_ERROR));
                }
                Ok(Self::duration_from_validated(secs, ease))
            }
            AnimationMotion::Spring(spring) => {
                if !spring.is_valid() {
                    return Err(D::Error::custom(SPRING_ERROR));
                }
                Ok(Self {
                    motion: AnimationMotion::Spring(spring),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
