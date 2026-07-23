//! Custom wire formats for primitive types.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use ::serde::de::{self, Error as _, IgnoredAny, MapAccess, SeqAccess, Visitor};
use ::serde::ser::{SerializeSeq, SerializeStruct};
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::primitives::color::{Color, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let ColorU8 { r, g, b, a } = self.to_srgb_u8();
        let hex = if a == 0xff {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        };
        serializer.serialize_str(&hex)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Cow::<'de, str>::deserialize(deserializer)?;
        parse_hex(raw.trim()).map_err(D::Error::custom)
    }
}

fn parse_hex(value: &str) -> Result<Color, &'static str> {
    let body = value.strip_prefix('#').unwrap_or(value);
    let parse_byte = |index: usize| -> Result<u8, &'static str> {
        u8::from_str_radix(&body[index..index + 2], 16).map_err(|_| "invalid hex digit")
    };
    match body.len() {
        6 => Ok(Color::rgb_u8(
            parse_byte(0)?,
            parse_byte(2)?,
            parse_byte(4)?,
        )),
        8 => Ok(Color::rgba_u8(
            parse_byte(0)?,
            parse_byte(2)?,
            parse_byte(4)?,
            parse_byte(6)?,
        )),
        _ => Err("expected #rrggbb or #rrggbbaa"),
    }
}

impl Serialize for Size {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let finite = |value: f32| value.is_finite().then_some(value);
        let mut state = serializer.serialize_struct("Size", 2)?;
        state.serialize_field("w", &finite(self.w))?;
        state.serialize_field("h", &finite(self.h))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct RawSize {
            w: Option<f32>,
            h: Option<f32>,
        }

        let raw = RawSize::deserialize(deserializer)?;
        Ok(Size::new(
            raw.w.unwrap_or(f32::INFINITY),
            raw.h.unwrap_or(f32::INFINITY),
        ))
    }
}

/// Per-type policy for the shared lane serde. Implementors are the
/// `[u16; 4]`-backed primitives whose four lanes carry domain meaning.
trait LaneCodec: Sized {
    /// Struct-form field names, in lane order. Must be length 4.
    const FIELDS: &'static [&'static str];

    fn from_lane_array(lanes: [f32; 4]) -> Self;
    fn to_lane_array(&self) -> [f32; 4];

    /// The 2-node shorthand for these lanes, when they collapse to
    /// one. Callers have already ruled out the all-equal (scalar) case.
    fn two_form(lanes: [f32; 4]) -> Option<[f32; 2]>;

    /// Expand a parsed 2-node array back to four lanes.
    fn expand_two(pair: [f32; 2]) -> [f32; 4];
}

impl LaneCodec for Corners {
    const FIELDS: &'static [&'static str] = &["tl", "tr", "br", "bl"];

    fn from_lane_array(lanes: [f32; 4]) -> Self {
        Self::new(lanes[0], lanes[1], lanes[2], lanes[3])
    }

    fn to_lane_array(&self) -> [f32; 4] {
        self.as_array()
    }

    fn two_form(lanes: [f32; 4]) -> Option<[f32; 2]> {
        (lanes[0] == lanes[1] && lanes[2] == lanes[3]).then_some([lanes[0], lanes[2]])
    }

    fn expand_two([top, bottom]: [f32; 2]) -> [f32; 4] {
        [top, top, bottom, bottom]
    }
}

impl LaneCodec for Spacing {
    const FIELDS: &'static [&'static str] = &["left", "top", "right", "bottom"];

    fn from_lane_array(lanes: [f32; 4]) -> Self {
        Self::new(lanes[0], lanes[1], lanes[2], lanes[3])
    }

    fn to_lane_array(&self) -> [f32; 4] {
        self.as_array()
    }

    fn two_form(lanes: [f32; 4]) -> Option<[f32; 2]> {
        (lanes[0] == lanes[2] && lanes[1] == lanes[3]).then_some([lanes[0], lanes[1]])
    }

    fn expand_two([horizontal, vertical]: [f32; 2]) -> [f32; 4] {
        [horizontal, vertical, horizontal, vertical]
    }
}

impl Serialize for Corners {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_lanes(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Corners {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_lanes(deserializer)
    }
}

impl Serialize for Spacing {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_lanes(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Spacing {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_lanes(deserializer)
    }
}

fn serialize_lanes<T: LaneCodec, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
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

fn deserialize_lanes<'de, T: LaneCodec, D: Deserializer<'de>>(d: D) -> Result<T, D::Error> {
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
    use ::serde::de::value::{Error, MapDeserializer, SeqDeserializer};

    use crate::primitives::color::Color;
    use crate::primitives::serde::{LaneCodec, deserialize_lanes, parse_hex};

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
        deserialize_lanes(deserializer)
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
                    "invalid length {}, expected a number, a 1-, 2-, or 4-node array, or a {{a, b, c, d}} table",
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
            deserialize_lanes::<TestLanes, _>(missing).unwrap(),
            TestLanes([1.0, 0.0, 3.0, 0.0]),
        );

        let duplicate = MapDeserializer::<_, Error>::new([("a", 1.0), ("a", 2.0)].into_iter());
        let error = deserialize_lanes::<TestLanes, _>(duplicate).unwrap_err();
        assert_eq!(error.to_string(), "duplicate field `a`");
    }

    #[test]
    fn color_parse_accepts_with_and_without_hash() {
        assert_eq!(
            parse_hex("#3266cc").unwrap(),
            Color::rgb_u8(0x32, 0x66, 0xcc)
        );
        assert_eq!(
            parse_hex("3266cc").unwrap(),
            Color::rgb_u8(0x32, 0x66, 0xcc)
        );
        assert_eq!(
            parse_hex("#3266cc80").unwrap(),
            Color::rgba_u8(0x32, 0x66, 0xcc, 0x80)
        );
    }

    #[test]
    fn color_parse_rejects_malformed_input() {
        assert!(parse_hex("").is_err());
        assert!(parse_hex("#").is_err());
        assert!(parse_hex("#abc").is_err(), "3-digit not supported");
        assert!(parse_hex("#abcde").is_err(), "5-digit not supported");
        assert!(parse_hex("#abcdefab12").is_err(), "10-digit too long");
        assert!(parse_hex("#zzzzzz").is_err(), "non-hex digits");
    }
}
