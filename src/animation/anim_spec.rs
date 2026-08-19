//! The authored animation spec: which motion model a value travels
//! under, and the parameters that model was authored with.

use crate::animation::duration::{DURATION_ERROR, duration_is_valid};
use crate::animation::easing::Easing;
use crate::animation::spring::{
    SPRING_ERROR, params_are_valid as spring_params_are_valid, stable_substep_dt,
};
use crate::common::time::ANIM_SUBSTEP_DT;
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AnimMotion {
    Duration {
        secs: f32,
        ease: Easing,
    },
    Spring {
        stiffness: f32,
        damping: f32,
        substep_dt: f32,
    },
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
    /// Near-critically-damped spring tuned as a general-purpose default.
    pub const SPRING: Self = Self {
        motion: AnimMotion::Spring {
            stiffness: 170.0,
            damping: 26.0,
            substep_dt: ANIM_SUBSTEP_DT,
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

    /// Construct a damped spring whose convergence rate and adaptive
    /// integration cost stay within the supported UI-animation domain.
    ///
    /// # Panics
    ///
    /// Panics when either parameter is non-positive/non-finite, the slowest
    /// decay rate is below 1/s, or a maximally clamped frame would require
    /// more than 256 integration substeps.
    pub fn spring(stiffness: f32, damping: f32) -> Self {
        let substep_dt = stable_substep_dt(stiffness, damping);
        assert!(
            spring_params_are_valid(stiffness, damping, substep_dt),
            "{SPRING_ERROR}"
        );
        Self::spring_from_validated(stiffness, damping, substep_dt)
    }

    fn spring_from_validated(stiffness: f32, damping: f32, substep_dt: f32) -> Self {
        Self {
            motion: AnimMotion::Spring {
                stiffness,
                damping,
                substep_dt,
            },
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

/// [`AnimSpec`]'s *authored* form — [`AnimMotion`] minus what
/// deserialization derives.
///
/// Not redundant with `AnimMotion`: `substep_dt` falls out of
/// `(stiffness, damping)` rather than being written by a theme author,
/// so it has no wire field, and `Deserialize` recomputes *and validates*
/// it. That validation is the reason this is a hand-written impl over a
/// separate type rather than `#[serde(skip)]` on the field — a skipped
/// field would deserialize to `0.0` and silently bypass `SPRING_ERROR`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AnimSpecWire {
    Duration { secs: f32, ease: Easing },
    Spring { stiffness: f32, damping: f32 },
}

impl Serialize for AnimSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match self.motion {
            AnimMotion::Duration { secs, ease } => AnimSpecWire::Duration { secs, ease },
            // `substep_dt: _`, not `..`: a field added to `Spring` is
            // then a compile error here, forcing the one decision that
            // matters — is it authored, or derived like this one?
            AnimMotion::Spring {
                stiffness,
                damping,
                substep_dt: _,
            } => AnimSpecWire::Spring { stiffness, damping },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AnimSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match AnimSpecWire::deserialize(deserializer)? {
            AnimSpecWire::Duration { secs, ease } => {
                if !duration_is_valid(secs) {
                    return Err(D::Error::custom(DURATION_ERROR));
                }
                Ok(Self::duration_from_validated(secs, ease))
            }
            AnimSpecWire::Spring { stiffness, damping } => {
                let substep_dt = stable_substep_dt(stiffness, damping);
                if !spring_params_are_valid(stiffness, damping, substep_dt) {
                    return Err(D::Error::custom(SPRING_ERROR));
                }
                Ok(Self::spring_from_validated(stiffness, damping, substep_dt))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::animation::anim_spec::AnimSpec;
    use crate::animation::easing::Easing;
    use crate::primitives::approx::EPS;

    #[test]
    fn anim_spec_construction_validates_and_canonicalizes() {
        let instant_zero = AnimSpec::duration(0.0, Easing::Linear);
        let instant_negative_zero = AnimSpec::duration(-0.0, Easing::Linear);
        let instant_sub_eps = AnimSpec::duration(EPS * 0.5, Easing::Linear);
        assert!(instant_zero.is_instant());
        assert!(instant_negative_zero.is_instant());
        assert!(instant_sub_eps.is_instant());
        assert!(!AnimSpec::duration(EPS, Easing::Linear).is_instant());
        assert!(!AnimSpec::duration(60.0, Easing::Linear).is_instant());
        assert!(!AnimSpec::FAST.is_instant());
        assert!(!AnimSpec::SPRING.is_instant());

        for secs in [
            -1.0,
            60.0 + f32::EPSILON * 64.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            assert!(
                std::panic::catch_unwind(|| AnimSpec::duration(secs, Easing::Linear)).is_err(),
                "duration constructor accepted {secs:?}",
            );
        }

        for (stiffness, damping) in [
            (0.0, 1.0),
            (1.0, 0.0),
            (-1.0, 1.0),
            (1.0, -1.0),
            (f32::NAN, 1.0),
            (1.0, f32::INFINITY),
            (1.0, 1.0),
            (1.0, 100.0),
            (f32::MAX, 2.0),
        ] {
            assert!(
                std::panic::catch_unwind(|| AnimSpec::spring(stiffness, damping)).is_err(),
                "spring constructor accepted ({stiffness:?}, {damping:?})",
            );
        }

        assert!(!AnimSpec::spring(1.0, 2.0).is_instant());
        assert!(!AnimSpec::spring(1_000_000.0, 100.0).is_instant());
    }

    #[test]
    fn anim_spec_serde_validates_and_roundtrips() {
        #[derive(::serde::Serialize, ::serde::Deserialize, PartialEq, Debug)]
        struct Holder {
            spec: AnimSpec,
        }
        let cases = [
            AnimSpec::FAST,
            AnimSpec::MEDIUM,
            AnimSpec::SPRING,
            AnimSpec::duration(0.1, Easing::Linear),
            AnimSpec::duration(0.2, Easing::InOutCubic),
            AnimSpec::duration(0.3, Easing::OutQuart),
            AnimSpec::duration(0.4, Easing::OutBack),
            AnimSpec::spring(100.0, 15.0),
            AnimSpec::spring(1_000_000.0, 100.0),
        ];
        for spec in cases {
            let h = Holder { spec };
            let s = toml::to_string(&h).expect("serialize");
            let back: Holder = toml::from_str(&s).expect("parse");
            assert_eq!(back, h, "roundtrip mismatch for {spec:?}\nTOML:\n{s}");
        }

        let canonical: Holder = toml::from_str(
            r#"
                [spec]
                kind = "duration"
                secs = 0.00005
                ease = "linear"
            "#,
        )
        .expect("sub-epsilon duration is a valid instant");
        assert!(canonical.spec.is_instant());
        assert!(
            toml::to_string(&canonical)
                .expect("serialize canonical duration")
                .contains("secs = 0.0"),
        );

        let invalid = [
            (
                "negative duration",
                r#"
                    [spec]
                    kind = "duration"
                    secs = -1.0
                    ease = "linear"
                "#,
                "animation duration must be finite and in 0.0..=60.0 seconds",
            ),
            (
                "non-finite duration",
                r#"
                    [spec]
                    kind = "duration"
                    secs = nan
                    ease = "linear"
                "#,
                "animation duration must be finite and in 0.0..=60.0 seconds",
            ),
            (
                "non-positive spring",
                r#"
                    [spec]
                    kind = "spring"
                    stiffness = 170.0
                    damping = 0.0
                "#,
                "spring parameters must be positive, finite, convergent, and within the integration limit",
            ),
            (
                "slow spring",
                r#"
                    [spec]
                    kind = "spring"
                    stiffness = 1.0
                    damping = 100.0
                "#,
                "spring parameters must be positive, finite, convergent, and within the integration limit",
            ),
            (
                "expensive spring",
                r#"
                    [spec]
                    kind = "spring"
                    stiffness = 3.4028235e38
                    damping = 2.0
                "#,
                "spring parameters must be positive, finite, convergent, and within the integration limit",
            ),
        ];
        for (label, input, expected) in invalid {
            let error = toml::from_str::<Holder>(input).expect_err(label);
            assert!(
                error.to_string().contains(expected),
                "{label}: unexpected serde error: {error}",
            );
        }
    }
}
