//! Shared compact serde for the f16x4 lane newtypes (`Corners`,
//! `Spacing`). Both serialize as a bare scalar (all four lanes equal),
//! a 2-element array (one symmetric collapse), or a 4-element array,
//! and parse those three forms plus a named-field struct table. They
//! differ only in the field names and in how a 2-element form maps
//! onto the four lanes — captured by [`LaneCodec`]; everything else is
//! shared here so the two visitors can't drift apart.

use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use std::fmt;
use std::marker::PhantomData;

/// Per-type policy for the shared lane serde. Implementors are the
/// `[u16; 4]`-backed primitives whose four lanes carry domain meaning.
pub(crate) trait LaneCodec: Sized {
    /// Struct-form field names, in lane order. Must be length 4.
    const FIELDS: &'static [&'static str];

    fn from_lane_array(lanes: [f32; 4]) -> Self;
    fn to_lane_array(&self) -> [f32; 4];

    /// The 2-element shorthand for these lanes, when they collapse to
    /// one. Callers have already ruled out the all-equal (scalar) case.
    fn two_form(lanes: [f32; 4]) -> Option<[f32; 2]>;

    /// Expand a parsed 2-element array back to four lanes.
    fn expand_two(pair: [f32; 2]) -> [f32; 4];
}

pub(crate) fn serialize<T: LaneCodec, S: serde::Serializer>(
    v: &T,
    s: S,
) -> Result<S::Ok, S::Error> {
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

pub(crate) fn deserialize<'de, T: LaneCodec, D: serde::Deserializer<'de>>(
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
            "a number, a 1-, 2-, or 4-element array, or a {{{}}} table",
            T::FIELDS.join(", ")
        )
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<T, E> {
        Ok(T::from_lane_array([v as f32; 4]))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<T, E> {
        Ok(T::from_lane_array([v as f32; 4]))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<T, E> {
        Ok(T::from_lane_array([v as f32; 4]))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<T, A::Error> {
        let v0: f32 = a
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let Some(v1) = a.next_element::<f32>()? else {
            return Ok(T::from_lane_array([v0; 4]));
        };
        let Some(v2) = a.next_element::<f32>()? else {
            return Ok(T::from_lane_array(T::expand_two([v0, v1])));
        };
        let v3: f32 = a
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(3, &self))?;
        if a.next_element::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(5, &self));
        }
        Ok(T::from_lane_array([v0, v1, v2, v3]))
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
        Ok(T::from_lane_array(lanes.map(|o| o.unwrap_or(0.0))))
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::lane_serde::{self, LaneCodec};
    use serde::de::value::{Error, MapDeserializer, SeqDeserializer};

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct TestLanes([f32; 4]);

    impl LaneCodec for TestLanes {
        const FIELDS: &'static [&'static str] = &["a", "b", "c", "d"];

        fn from_lane_array(lanes: [f32; 4]) -> Self {
            Self(lanes)
        }

        fn to_lane_array(&self) -> [f32; 4] {
            self.0
        }

        fn two_form(_lanes: [f32; 4]) -> Option<[f32; 2]> {
            None
        }

        fn expand_two([a, b]: [f32; 2]) -> [f32; 4] {
            [a, a, b, b]
        }
    }

    fn deserialize_seq(values: &[f32]) -> Result<TestLanes, Error> {
        let deserializer = SeqDeserializer::new(values.iter().copied());
        lane_serde::deserialize(deserializer)
    }

    #[test]
    fn sequence_lengths_preserve_supported_forms_and_reject_others() {
        assert_eq!(deserialize_seq(&[4.0]).unwrap(), TestLanes([4.0; 4]));
        assert_eq!(
            deserialize_seq(&[1.0, 2.0]).unwrap(),
            TestLanes([1.0, 1.0, 2.0, 2.0]),
        );
        assert_eq!(
            deserialize_seq(&[1.0, 2.0, 3.0, 4.0]).unwrap(),
            TestLanes([1.0, 2.0, 3.0, 4.0]),
        );

        for values in [&[][..], &[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0, 4.0, 5.0]] {
            let error = deserialize_seq(values).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "invalid length {}, expected a number, a 1-, 2-, or 4-element array, or a {{a, b, c, d}} table",
                    values.len(),
                ),
                "values={values:?}",
            );
        }
    }

    #[test]
    fn map_preserves_missing_defaults_and_rejects_duplicate_fields() {
        let missing = MapDeserializer::<_, Error>::new([("a", 1.0), ("c", 3.0)].into_iter());
        assert_eq!(
            lane_serde::deserialize::<TestLanes, _>(missing).unwrap(),
            TestLanes([1.0, 0.0, 3.0, 0.0]),
        );

        let duplicate = MapDeserializer::<_, Error>::new([("a", 1.0), ("a", 2.0)].into_iter());
        let error = lane_serde::deserialize::<TestLanes, _>(duplicate).unwrap_err();
        assert_eq!(error.to_string(), "duplicate field `a`");
    }
}
