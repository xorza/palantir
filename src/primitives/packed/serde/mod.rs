//! The shared four-lane wire format: a number, a 1- or 4-node array, a 2-node array for a type
//! that takes one ([`TwoNode`]), or a named table.
//! [`Corners`](crate::primitives::geometry::corners::Corners) and
//! [`Spacing`](crate::primitives::geometry::spacing::Spacing) differ in lane names and pairing,
//! which [`LaneCodec`] captures; the serde impls live beside each type.

use std::fmt;
use std::marker::PhantomData;

use ::serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use ::serde::ser::SerializeSeq;
use ::serde::{Deserializer, Serializer};

/// Per-type policy for the shared lane serde, for `[u16; 4]`-backed primitives with four meaningful lanes.
///
/// **An omitted table lane reads as `0.0`**, the identity every type here shares. A type with another neutral (`Size`: unbounded) writes its own serde. The table must still name a lane.
pub(crate) trait LaneCodec: Sized {
    /// Struct-form field names, in lane order. Must be length 4.
    const FIELDS: &'static [&'static str];

    /// The lanes packed, each one [`Self::lane_is_valid`] holds.
    fn from_valid_lanes(lanes: [f32; 4]) -> Self;
    fn to_lane_array(&self) -> [f32; 4];

    const TWO_NODE: TwoNode;

    /// Whether a parsed lane is holdable; failures are deserialization errors, not later NaN or f16 overflow.
    fn lane_is_valid(lane: f32) -> bool;

    /// What [`Self::lane_is_valid`] demands, for the error message.
    const LANE_RULE: &'static str;
}

/// What a 2-node array means for a lane type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TwoNode {
    /// Refused, for a type whose pairs readers read more than one way.
    Refused,
    /// `[a, b]` is lanes `[a, b, a, b]`: the first and third lanes take `a`, the second and
    /// fourth `b`.
    Alternate,
}

/// Build `T` from parsed lanes, refusing any lane `T` cannot hold.
fn checked_lanes<T: LaneCodec, E: de::Error>(lanes: [f32; 4]) -> Result<T, E> {
    match lanes.into_iter().find(|lane| !T::lane_is_valid(*lane)) {
        Some(bad) => Err(E::custom(format_args!("{}, got {bad}", T::LANE_RULE))),
        None => Ok(T::from_valid_lanes(lanes)),
    }
}

pub(crate) fn serialize_lanes<T: LaneCodec, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    let lanes = v.to_lane_array();
    let [a, b, c, d] = lanes;
    if a == b && b == c && c == d {
        return s.serialize_f32(a);
    }
    if T::TWO_NODE == TwoNode::Alternate && a == c && b == d {
        let mut seq = s.serialize_seq(Some(2))?;
        seq.serialize_element(&a)?;
        seq.serialize_element(&b)?;
        return seq.end();
    }
    let mut seq = s.serialize_seq(Some(4))?;
    for x in lanes {
        seq.serialize_element(&x)?;
    }
    seq.end()
}

pub(crate) fn deserialize_lanes<'de, T: LaneCodec, D: Deserializer<'de>>(
    d: D,
) -> Result<T, D::Error> {
    d.deserialize_any(LaneVisitor::<T>(PhantomData))
}

#[derive(Debug)]
struct LaneVisitor<T>(PhantomData<T>);

impl<'de, T: LaneCodec> Visitor<'de> for LaneVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let arrays = match T::TWO_NODE {
            TwoNode::Refused => "a 1- or 4-node array",
            TwoNode::Alternate => "a 1-, 2-, or 4-node array",
        };
        write!(
            f,
            "a number, {arrays}, or a {{{}}} table",
            T::FIELDS.join(", ")
        )
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<T, E> {
        checked_lanes([v as f32; 4])
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<T, E> {
        checked_lanes([v as f32; 4])
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<T, E> {
        checked_lanes([v as f32; 4])
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<T, A::Error> {
        let v0: f32 = a
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let Some(v1) = a.next_element::<f32>()? else {
            return checked_lanes([v0; 4]);
        };
        let Some(v2) = a.next_element::<f32>()? else {
            return match T::TWO_NODE {
                TwoNode::Refused => Err(de::Error::invalid_length(2, &self)),
                TwoNode::Alternate => checked_lanes([v0, v1, v0, v1]),
            };
        };
        let v3: f32 = a
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(3, &self))?;
        if a.next_element::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(5, &self));
        }
        checked_lanes([v0, v1, v2, v3])
    }

    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<T, A::Error> {
        let mut lanes = [None::<f32>; 4];
        while let Some(k) = m.next_key::<String>()? {
            match T::FIELDS.iter().position(|f| *f == k) {
                Some(i) => {
                    if lanes[i].is_some() {
                        return Err(de::Error::duplicate_field(T::FIELDS[i]));
                    }
                    lanes[i] = Some(m.next_value()?);
                }
                None => return Err(de::Error::unknown_field(&k, T::FIELDS)),
            }
        }
        if lanes.iter().all(Option::is_none) {
            return Err(de::Error::invalid_length(0, &self));
        }
        checked_lanes(lanes.map(|o| o.unwrap_or(0.0)))
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use ron::ser;
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    /// `value` in RON, as a theme file spells it.
    pub(crate) fn ron_text<T: Serialize>(value: &T) -> String {
        ser::to_string(value).expect("serialize")
    }

    /// `text` read as RON into a `T`.
    pub(crate) fn from_ron<T: DeserializeOwned>(text: &str) -> T {
        ron::from_str(text).expect("parse")
    }
}

#[cfg(test)]
mod tests;
