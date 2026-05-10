use super::num::Num;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Spacing {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
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
        if self.left == self.right && self.top == self.bottom && self.left == self.top {
            return s.serialize_f32(self.left);
        }
        if self.left == self.right && self.top == self.bottom {
            let mut seq = s.serialize_seq(Some(2))?;
            seq.serialize_element(&self.left)?;
            seq.serialize_element(&self.top)?;
            return seq.end();
        }
        let mut seq = s.serialize_seq(Some(4))?;
        seq.serialize_element(&self.left)?;
        seq.serialize_element(&self.top)?;
        seq.serialize_element(&self.right)?;
        seq.serialize_element(&self.bottom)?;
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
                Ok(Spacing {
                    left: v0,
                    top: v1,
                    right: v2,
                    bottom: v3,
                })
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
                Ok(Spacing {
                    left: left.unwrap_or(0.0),
                    top: top.unwrap_or(0.0),
                    right: right.unwrap_or(0.0),
                    bottom: bottom.unwrap_or(0.0),
                })
            }
        }
        d.deserialize_any(V)
    }
}

impl std::hash::Hash for Spacing {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl Spacing {
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
    pub const fn all(v: f32) -> Self {
        Self {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }
    pub const fn xy(x: f32, y: f32) -> Self {
        Self {
            left: x,
            top: y,
            right: x,
            bottom: y,
        }
    }
    pub const fn horiz(&self) -> f32 {
        self.left + self.right
    }
    pub const fn vert(&self) -> f32 {
        self.top + self.bottom
    }
}

impl std::ops::Add for Spacing {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            left: self.left + rhs.left,
            top: self.top + rhs.top,
            right: self.right + rhs.right,
            bottom: self.bottom + rhs.bottom,
        }
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
        Self {
            left: l.as_f32(),
            top: t.as_f32(),
            right: r.as_f32(),
            bottom: b.as_f32(),
        }
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
    fn uniform_spacing_emits_scalar() {
        assert_eq!(ser(Spacing::all(4.0)).trim(), "v = 4.0");
    }

    #[test]
    fn axis_pair_emits_two_element_array() {
        // left=right=horizontal, top=bottom=vertical, h != v.
        assert_eq!(ser(Spacing::xy(4.0, 8.0)).trim(), "v = [4.0, 8.0]");
    }

    #[test]
    fn asymmetric_emits_four_element_array() {
        let s = Spacing {
            left: 1.0,
            top: 2.0,
            right: 3.0,
            bottom: 4.0,
        };
        assert_eq!(ser(s).trim(), "v = [1.0, 2.0, 3.0, 4.0]");
    }

    /// "Matched pair" check is exact equality. left==top and right==bottom but
    /// left!=right must NOT collapse to the 2-array form (would lose data).
    #[test]
    fn diagonal_match_does_not_collapse() {
        let s = Spacing {
            left: 1.0,
            top: 1.0,
            right: 2.0,
            bottom: 2.0,
        };
        assert_eq!(ser(s).trim(), "v = [1.0, 1.0, 2.0, 2.0]");
    }

    #[test]
    fn deserialize_scalar_form() {
        assert_eq!(de("v = 4.0"), Spacing::all(4.0));
    }

    #[test]
    fn deserialize_integer_scalar_via_visit_i64() {
        assert_eq!(de("v = 4"), Spacing::all(4.0));
    }

    #[test]
    fn deserialize_two_element_array() {
        assert_eq!(de("v = [4.0, 8.0]"), Spacing::xy(4.0, 8.0));
    }

    #[test]
    fn deserialize_four_element_array() {
        assert_eq!(
            de("v = [1.0, 2.0, 3.0, 4.0]"),
            Spacing {
                left: 1.0,
                top: 2.0,
                right: 3.0,
                bottom: 4.0,
            }
        );
    }

    #[test]
    fn deserialize_one_element_array_treated_as_uniform() {
        assert_eq!(de("v = [4.0]"), Spacing::all(4.0));
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
        assert_eq!(
            de(toml_str),
            Spacing {
                left: 1.0,
                top: 2.0,
                right: 3.0,
                bottom: 4.0,
            }
        );
    }

    #[test]
    fn deserialize_struct_form_with_missing_fields_defaults_to_zero() {
        let toml_str = r#"
[v]
left = 4.0
right = 4.0
"#;
        assert_eq!(
            de(toml_str),
            Spacing {
                left: 4.0,
                top: 0.0,
                right: 4.0,
                bottom: 0.0,
            }
        );
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
            Spacing {
                left: 1.0,
                top: 2.0,
                right: 3.0,
                bottom: 4.0,
            },
        ] {
            let out = ser(s);
            assert_eq!(de(&out), s, "round-trip failed for {s:?} -> {out}");
        }
    }
}
