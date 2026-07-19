use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::animation::easing::Easing;
use crate::animation::spring::{params_are_valid as spring_params_are_valid, stable_substep_dt};
use crate::animation::{AnimMotion, AnimSpec, DURATION_ERROR, SPRING_ERROR, duration_is_valid};

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
            AnimMotion::Spring {
                stiffness, damping, ..
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
