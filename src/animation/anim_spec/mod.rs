//! The authored animation spec: which motion model a value travels
//! under, and the parameters that model was authored with.

use crate::animation::duration::{DURATION_ERROR, duration_is_valid};
use crate::animation::easing::Easing;
use crate::animation::spring::{SPRING_ERROR, params_are_valid as spring_params_are_valid};
use crate::primitives::approx::EPS;
use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How a value moves toward its target. Animation itself is opt-in
/// at the call site — pass `None` to [`crate::Ui::animate`] (or omit
/// the field on a theme) when you want snap-to-target behavior.
/// `AnimSpec` only describes what motion looks like *when there is
/// motion*; "no animation" lives in `Option<AnimSpec>`, not as a
/// variant here.
///
/// Wire format is internally tagged on `kind` (snake_case), so theme
/// files read cleanly:
///
/// ```toml
/// [theme.button.anim]
/// kind = "duration"
/// secs = 0.12
/// ease = "out_cubic"
///
/// [theme.button.anim]
/// kind = "spring"
/// stiffness = 170.0
/// damping = 26.0
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimSpec {
    pub(super) motion: AnimMotion,
}

/// The motion model a spec was authored under, plus its
/// parameters. Kept private to the module: the public surface is
/// [`AnimSpec`]'s constructors, and every reader is an animation-row
/// step that matches on it.
///
/// Also the wire shape — every field here is authored, so
/// [`AnimSpec`]'s hand-written impls delegate to this one and spend
/// themselves on validation alone.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum AnimMotion {
    Duration { secs: f32, ease: Easing },
    Spring { stiffness: f32, damping: f32 },
}

impl AnimSpec {
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

    /// Construct a duration animation. Values below `1e-4` canonicalize to an
    /// instant snap.
    ///
    /// # Panics
    ///
    /// Panics unless `secs` is finite and in `0.0..=60.0`.
    pub const fn duration(secs: f32, ease: Easing) -> Self {
        // Spelled out rather than `"{DURATION_ERROR}"`: a `const fn`
        // cannot run the formatting machinery interpolation needs.
        assert!(
            duration_is_valid(secs),
            "animation duration must be finite and in 0.0..=60.0 seconds"
        );
        Self::duration_from_validated(secs, ease)
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
    /// spring *arrives* and then stops: it must decay at 1/s or faster,
    /// and it must not be so stiff that its residual velocity keeps the
    /// value unsettled long after the motion stopped being visible.
    ///
    /// # Panics
    ///
    /// Panics when either parameter is non-positive or non-finite, when
    /// the slowest decay rate is below 1/s, or when that decay would
    /// take more than 4 s to bring the velocity down to its settle
    /// floor. Raise `damping` or lower `stiffness` for the last one.
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
    pub fn is_instant(self) -> bool {
        match self.motion {
            AnimMotion::Duration { secs, .. } => secs == 0.0,
            AnimMotion::Spring { .. } => false,
        }
    }
}

impl Serialize for AnimSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.motion.serialize(serializer)
    }
}

/// Validating, so a hand-written impl rather than `#[serde(transparent)]`:
/// a theme file is untrusted input, and the bounds [`AnimSpec::duration`]
/// and [`AnimSpec::spring`] assert on have to hold for a spec that arrived
/// over the wire too. Bad data is an `Err` here rather than the panic those
/// two raise.
impl<'de> Deserialize<'de> for AnimSpec {
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
