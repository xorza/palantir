use ::serde::de::Error as _;
use ::serde::ser::SerializeStruct;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};
use tinyvec::ArrayVec;

use crate::primitives::brush::gradient::stops::{GradientStops, MAX_STOPS, Stop};
use crate::primitives::color::ColorU8;

impl Serialize for Stop {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Stop", 2)?;
        state.serialize_field("offset", &self.offset())?;
        state.serialize_field("color", &self.color)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Stop {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct RawStop {
            offset: f32,
            color: ColorU8,
        }

        let raw = RawStop::deserialize(deserializer)?;
        if !raw.offset.is_finite() {
            return Err(D::Error::custom("gradient stop offset must be finite"));
        }
        Ok(Stop::new(raw.offset, raw.color))
    }
}

impl Serialize for GradientStops {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GradientStops {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = ArrayVec::<[Stop; MAX_STOPS]>::deserialize(deserializer)?;
        if values.len() < 2 {
            return Err(D::Error::custom(format_args!(
                "gradient requires at least 2 stops, got {}",
                values.len(),
            )));
        }
        Ok(Self(values))
    }
}
