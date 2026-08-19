use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::animation::easing::Easing;
use crate::animation::spring::{params_are_valid as spring_params_are_valid, stable_substep_dt};
use crate::animation::{AnimMotion, AnimSpec, DURATION_ERROR, SPRING_ERROR, duration_is_valid};

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
