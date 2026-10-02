//! The shared four-lane wire format.
//!
//! [`Corners`](crate::primitives::corners::Corners) and
//! [`Spacing`](crate::primitives::spacing::Spacing) both serialize as
//! "a number, a 1-, 2-, or 4-node array, or a named table", differing
//! only in what their lanes are called and how the 2-node shorthand
//! expands. That policy is [`LaneCodec`], implemented next to each type;
//! the `Serialize` / `Deserialize` impls that drive it live there too.
//! What stays here is the machinery neither type owns.

pub(crate) mod checked;

use std::fmt;
use std::marker::PhantomData;

use ::serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use ::serde::ser::SerializeSeq;
use ::serde::{Deserializer, Serializer};

/// Per-type policy for the shared lane serde. Implementors are the
/// `[u16; 4]`-backed primitives whose four lanes carry domain meaning.
///
/// **A lane the table form omits reads as `0.0`** — the identity every
/// type on this codec shares, so `{tl: 4}` is a radius on one corner
/// and nothing on the others. A type whose neutral is something else
/// does not belong here: `Size` reads an omitted axis as *unbounded*,
/// and writes its own serde for that one reason. The table must still
/// name a lane, matching the array form's rejection of an empty node.
pub(super) trait LaneCodec: Sized {
    /// Struct-form field names, in lane order. Must be length 4.
    const FIELDS: &'static [&'static str];

    fn from_lane_array(lanes: [f32; 4]) -> Self;
    fn to_lane_array(&self) -> [f32; 4];

    /// The 2-node shorthand for these lanes, when they collapse to
    /// one. Callers have already ruled out the all-equal (scalar) case.
    fn two_form(lanes: [f32; 4]) -> Option<[f32; 2]>;

    /// Expand a parsed 2-node array back to four lanes.
    fn expand_two(pair: [f32; 2]) -> [f32; 4];

    /// Whether one parsed lane is a value this type can hold. A file is
    /// untrusted, so a lane that fails is a deserialization error rather
    /// than a value that reaches a NaN screen or an f16 overflow later.
    fn lane_is_valid(lane: f32) -> bool;

    /// What [`Self::lane_is_valid`] demands, for the error message.
    const LANE_RULE: &'static str;
}

/// Build `T` from parsed lanes, refusing any lane `T` cannot hold.
fn checked_lanes<T: LaneCodec, E: de::Error>(lanes: [f32; 4]) -> Result<T, E> {
    match lanes.into_iter().find(|lane| !T::lane_is_valid(*lane)) {
        Some(bad) => Err(E::custom(format_args!("{}, got {bad}", T::LANE_RULE))),
        None => Ok(T::from_lane_array(lanes)),
    }
}

pub(super) fn serialize_lanes<T: LaneCodec, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    let lanes = v.to_lane_array();
    let [a, b, c, d] = lanes;
    if a == b && b == c && c == d {
        return s.serialize_f32(a);
    }
    if let Some([p, q]) = T::two_form(lanes) {
        let mut seq = s.serialize_seq(Some(2))?;
        seq.serialize_element(&p)?;
        seq.serialize_element(&q)?;
        return seq.end();
    }
    let mut seq = s.serialize_seq(Some(4))?;
    for x in lanes {
        seq.serialize_element(&x)?;
    }
    seq.end()
}

pub(super) fn deserialize_lanes<'de, T: LaneCodec, D: Deserializer<'de>>(
    d: D,
) -> Result<T, D::Error> {
    d.deserialize_any(LaneVisitor::<T>(PhantomData))
}

#[derive(Debug)]
struct LaneVisitor<T>(PhantomData<T>);

impl<'de, T: LaneCodec> Visitor<'de> for LaneVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "a number, a 1-, 2-, or 4-node array, or a {{{}}} table",
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
            return checked_lanes(T::expand_two([v0, v1]));
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
pub(crate) mod test_support {
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    /// `value` in RON, as a theme file spells it.
    pub(crate) fn ron_text<T: Serialize>(value: &T) -> String {
        ron::ser::to_string(value).expect("serialize")
    }

    /// `text` read as RON into a `T`.
    pub(crate) fn from_ron<T: DeserializeOwned>(text: &str) -> T {
        ron::from_str(text).expect("parse")
    }
}

#[cfg(test)]
mod tests;
