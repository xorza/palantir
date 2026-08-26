use ::serde::de::value::{Error, MapDeserializer, SeqDeserializer};

use crate::primitives::serde::{LaneCodec, deserialize_lanes};

/// A codec with neutral lane names, so these tests pin the shared
/// machinery rather than either real type's field spellings. The
/// per-type halves — which 2-node shorthand a type emits and expands,
/// and what its lanes are called — are pinned beside those types.
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

/// The three map outcomes, all decided by the shared visitor: an
/// absent lane defaults, a repeated one is a duplicate, and a name
/// outside `FIELDS` is rejected rather than ignored — the last is
/// what stops a typo in a theme file from silently reading as zero.
#[test]
fn map_defaults_missing_lanes_and_rejects_duplicate_or_unknown_fields() {
    let missing = MapDeserializer::<_, Error>::new([("a", 1.0), ("c", 3.0)].into_iter());
    assert_eq!(
        deserialize_lanes::<TestLanes, _>(missing).unwrap(),
        TestLanes([1.0, 0.0, 3.0, 0.0]),
    );

    let duplicate = MapDeserializer::<_, Error>::new([("a", 1.0), ("a", 2.0)].into_iter());
    let error = deserialize_lanes::<TestLanes, _>(duplicate).unwrap_err();
    assert_eq!(error.to_string(), "duplicate field `a`");

    let unknown = MapDeserializer::<_, Error>::new([("a", 1.0), ("typo", 2.0)].into_iter());
    let error = deserialize_lanes::<TestLanes, _>(unknown).unwrap_err();
    assert_eq!(
        error.to_string(),
        "unknown field `typo`, expected one of `a`, `b`, `c`, `d`",
    );
}
