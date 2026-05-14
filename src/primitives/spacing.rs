use super::num::Num;
use half::f16;

/// Per-side spacing (padding / margin), packed as four f16 lanes in
/// `[u16; 4]` (8 bytes). Lane order: `left | top | right | bottom`.
///
/// Precision: lossless for integer values up to 2048, ~0.25 px error
/// at 4096. UI spacing never approaches the f16 ceiling.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Spacing([u16; 4]);

impl std::fmt::Debug for Spacing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spacing")
            .field("left", &self.left())
            .field("top", &self.top())
            .field("right", &self.right())
            .field("bottom", &self.bottom())
            .finish()
    }
}

const fn pack(left: f32, top: f32, right: f32, bottom: f32) -> [u16; 4] {
    [
        f16::from_f32_const(left).to_bits(),
        f16::from_f32_const(top).to_bits(),
        f16::from_f32_const(right).to_bits(),
        f16::from_f32_const(bottom).to_bits(),
    ]
}

#[inline]
fn lane(bits: u16) -> f32 {
    f16::from_bits(bits).to_f32()
}

// Serialize Spacing compactly:
// - all four equal              → bare scalar `4.0`
// - left=right, top=bottom      → 2-element array `[horizontal, vertical]`
// - otherwise                   → 4-element array `[left, top, right, bottom]`
// Deserialize accepts all three forms plus the original struct shape
// `{ left, top, right, bottom }` for backward-compat with hand-written configs.
impl serde::Serialize for Spacing {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let (l, t, r, b) = (self.left(), self.top(), self.right(), self.bottom());
        if l == r && t == b && l == t {
            return s.serialize_f32(l);
        }
        if l == r && t == b {
            let mut seq = s.serialize_seq(Some(2))?;
            seq.serialize_element(&l)?;
            seq.serialize_element(&t)?;
            return seq.end();
        }
        let mut seq = s.serialize_seq(Some(4))?;
        seq.serialize_element(&l)?;
        seq.serialize_element(&t)?;
        seq.serialize_element(&r)?;
        seq.serialize_element(&b)?;
        seq.end()
    }
}

impl<'de> serde::Deserialize<'de> for Spacing {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Spacing;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a number, a [horizontal, vertical] or [left, top, right, bottom] array, or a {left, top, right, bottom} table")
            }

            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Spacing, E> {
                Ok(Spacing::all(v as f32))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Spacing, E> {
                Ok(Spacing::all(v as f32))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Spacing, E> {
                Ok(Spacing::all(v as f32))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Spacing, A::Error> {
                let v0: f32 = a
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let Some(v1) = a.next_element::<f32>()? else {
                    return Ok(Spacing::all(v0));
                };
                let Some(v2) = a.next_element::<f32>()? else {
                    return Ok(Spacing::xy(v0, v1));
                };
                let v3: f32 = a
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;
                Ok(Spacing::new(v0, v1, v2, v3))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut m: A,
            ) -> Result<Spacing, A::Error> {
                let mut left = None;
                let mut top = None;
                let mut right = None;
                let mut bottom = None;
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "left" => left = Some(m.next_value()?),
                        "top" => top = Some(m.next_value()?),
                        "right" => right = Some(m.next_value()?),
                        "bottom" => bottom = Some(m.next_value()?),
                        other => {
                            return Err(serde::de::Error::unknown_field(
                                other,
                                &["left", "top", "right", "bottom"],
                            ));
                        }
                    }
                }
                Ok(Spacing::new(
                    left.unwrap_or(0.0),
                    top.unwrap_or(0.0),
                    right.unwrap_or(0.0),
                    bottom.unwrap_or(0.0),
                ))
            }
        }
        d.deserialize_any(V)
    }
}

impl Spacing {
    pub const ZERO: Self = Self([0; 4]);

    pub const fn all(v: f32) -> Self {
        Self(pack(v, v, v, v))
    }

    pub const fn xy(x: f32, y: f32) -> Self {
        Self(pack(x, y, x, y))
    }

    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self(pack(left, top, right, bottom))
    }

    #[inline]
    pub fn left(&self) -> f32 {
        lane(self.0[0])
    }
    #[inline]
    pub fn top(&self) -> f32 {
        lane(self.0[1])
    }
    #[inline]
    pub fn right(&self) -> f32 {
        lane(self.0[2])
    }
    #[inline]
    pub fn bottom(&self) -> f32 {
        lane(self.0[3])
    }

    /// All four lanes unpacked at once. Routes through `half`'s
    /// platform-specific batched f16→f32 path (single `fcvtl` on
    /// aarch64-fp16, `vcvtph2ps` on x86-f16c, scalar fallback elsewhere).
    /// Use at hot sites that read 3+ lanes to amortize feature dispatch.
    #[inline]
    pub fn as_array(&self) -> [f32; 4] {
        use half::slice::HalfFloatSliceExt;
        let arr: &[half::f16; 4] = bytemuck::cast_ref(&self.0);
        let mut out = [0.0f32; 4];
        arr.as_slice().convert_to_f32_slice(&mut out);
        out
    }

    #[inline]
    pub fn horiz(&self) -> f32 {
        let [l, _t, r, _b] = self.as_array();
        l + r
    }
    #[inline]
    pub fn vert(&self) -> f32 {
        let [_l, t, _r, b] = self.as_array();
        t + b
    }
}

impl std::ops::Add for Spacing {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(
            self.left() + rhs.left(),
            self.top() + rhs.top(),
            self.right() + rhs.right(),
            self.bottom() + rhs.bottom(),
        )
    }
}

impl<T: Num> From<T> for Spacing {
    fn from(v: T) -> Self {
        Self::all(v.as_f32())
    }
}

/// `(horizontal, vertical)` — both sides on each axis.
impl<X: Num, Y: Num> From<(X, Y)> for Spacing {
    fn from((x, y): (X, Y)) -> Self {
        Self::xy(x.as_f32(), y.as_f32())
    }
}

/// `(left, top, right, bottom)` — matches struct field order.
impl<L: Num, T: Num, R: Num, B: Num> From<(L, T, R, B)> for Spacing {
    fn from((l, t, r, b): (L, T, R, B)) -> Self {
        Self::new(l.as_f32(), t.as_f32(), r.as_f32(), b.as_f32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ser(s: Spacing) -> String {
        #[derive(serde::Serialize)]
        struct W {
            v: Spacing,
        }
        toml::to_string(&W { v: s }).expect("serialize")
    }

    fn de(toml_str: &str) -> Spacing {
        #[derive(serde::Deserialize)]
        struct W {
            v: Spacing,
        }
        toml::from_str::<W>(toml_str).expect("parse").v
    }

    #[test]
    fn struct_is_eight_bytes() {
        assert_eq!(std::mem::size_of::<Spacing>(), 8);
    }

    #[test]
    fn lanes_round_trip_integer_values_exactly() {
        let s = Spacing::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(s.left(), 1.0);
        assert_eq!(s.top(), 2.0);
        assert_eq!(s.right(), 3.0);
        assert_eq!(s.bottom(), 4.0);
        assert_eq!(s.horiz(), 4.0);
        assert_eq!(s.vert(), 6.0);
    }

    #[test]
    fn serialize_picks_compact_form_per_symmetry() {
        let cases: &[(&str, Spacing, &str)] = &[
            ("uniform_scalar", Spacing::all(4.0), "v = 4.0"),
            (
                "axis_pair_two_array",
                Spacing::xy(4.0, 8.0),
                "v = [4.0, 8.0]",
            ),
            (
                "asymmetric_four_array",
                Spacing::new(1.0, 2.0, 3.0, 4.0),
                "v = [1.0, 2.0, 3.0, 4.0]",
            ),
            (
                "diagonal_match_does_not_collapse",
                Spacing::new(1.0, 1.0, 2.0, 2.0),
                "v = [1.0, 1.0, 2.0, 2.0]",
            ),
        ];
        for (label, s, want) in cases {
            assert_eq!(ser(*s).trim(), *want, "case: {label}");
        }
    }

    #[test]
    fn deserialize_accepts_scalar_array_and_integer_forms() {
        let cases: &[(&str, &str, Spacing)] = &[
            ("scalar", "v = 4.0", Spacing::all(4.0)),
            ("integer_scalar", "v = 4", Spacing::all(4.0)),
            ("two_element_array", "v = [4.0, 8.0]", Spacing::xy(4.0, 8.0)),
            (
                "four_element_array",
                "v = [1.0, 2.0, 3.0, 4.0]",
                Spacing::new(1.0, 2.0, 3.0, 4.0),
            ),
            ("one_element_array_uniform", "v = [4.0]", Spacing::all(4.0)),
        ];
        for (label, input, want) in cases {
            assert_eq!(de(input), *want, "case: {label}");
        }
    }

    #[test]
    fn deserialize_struct_form() {
        let toml_str = r#"
[v]
left = 1.0
top = 2.0
right = 3.0
bottom = 4.0
"#;
        assert_eq!(de(toml_str), Spacing::new(1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn deserialize_struct_form_with_missing_fields_defaults_to_zero() {
        let toml_str = r#"
[v]
left = 4.0
right = 4.0
"#;
        assert_eq!(de(toml_str), Spacing::new(4.0, 0.0, 4.0, 0.0));
    }

    #[test]
    fn deserialize_rejects_unknown_field() {
        #[derive(serde::Deserialize)]
        struct W {
            #[allow(dead_code)]
            v: Spacing,
        }
        let result: Result<W, _> = toml::from_str(
            r#"
[v]
left = 1.0
typo = 2.0
"#,
        );
        assert!(result.is_err(), "unknown field should be rejected");
    }

    #[test]
    fn serialize_then_parse_round_trips() {
        for s in [
            Spacing::all(4.0),
            Spacing::xy(4.0, 8.0),
            Spacing::new(1.0, 2.0, 3.0, 4.0),
        ] {
            let out = ser(s);
            assert_eq!(de(&out), s, "round-trip failed for {s:?} -> {out}");
        }
    }

    #[test]
    fn add_op() {
        let a = Spacing::new(1.0, 2.0, 3.0, 4.0);
        let b = Spacing::new(10.0, 20.0, 30.0, 40.0);
        let c = a + b;
        assert_eq!(c.left(), 11.0);
        assert_eq!(c.top(), 22.0);
        assert_eq!(c.right(), 33.0);
        assert_eq!(c.bottom(), 44.0);
    }
}
