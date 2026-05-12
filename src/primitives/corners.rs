use super::num::Num;
use super::size::Size;
use glam::Vec2;

/// Per-corner radii. `Vec2`/`Size` map to (top, bottom) pairs; `f32` is uniform.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Corners {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

impl std::hash::Hash for Corners {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

// Serialize Corners compactly:
// - all four equal     → bare scalar `4.0`
// - tl=tr, br=bl       → 2-element array `[top, bottom]` (CSS-style shorthand)
// - otherwise          → 4-element array `[tl, tr, br, bl]`
// Deserialize accepts all three forms plus the original struct shape
// `{ tl, tr, br, bl }` for backward-compat with hand-written configs.
impl serde::Serialize for Corners {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        if self.tl == self.tr && self.tr == self.br && self.br == self.bl {
            return s.serialize_f32(self.tl);
        }
        if self.tl == self.tr && self.br == self.bl {
            let mut seq = s.serialize_seq(Some(2))?;
            seq.serialize_element(&self.tl)?;
            seq.serialize_element(&self.br)?;
            return seq.end();
        }
        let mut seq = s.serialize_seq(Some(4))?;
        seq.serialize_element(&self.tl)?;
        seq.serialize_element(&self.tr)?;
        seq.serialize_element(&self.br)?;
        seq.serialize_element(&self.bl)?;
        seq.end()
    }
}

impl<'de> serde::Deserialize<'de> for Corners {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Corners;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a number, a [top, bottom] or [tl, tr, br, bl] array, or a {tl, tr, br, bl} table")
            }

            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Corners, E> {
                Ok(Corners::all(v as f32))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Corners, E> {
                Ok(Corners::all(v as f32))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Corners, E> {
                Ok(Corners::all(v as f32))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Corners, A::Error> {
                let v0: f32 = a
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let Some(v1) = a.next_element::<f32>()? else {
                    return Ok(Corners::all(v0));
                };
                let Some(v2) = a.next_element::<f32>()? else {
                    return Ok(Corners {
                        tl: v0,
                        tr: v0,
                        br: v1,
                        bl: v1,
                    });
                };
                let v3: f32 = a
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;
                Ok(Corners {
                    tl: v0,
                    tr: v1,
                    br: v2,
                    bl: v3,
                })
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut m: A,
            ) -> Result<Corners, A::Error> {
                let mut tl = None;
                let mut tr = None;
                let mut br = None;
                let mut bl = None;
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "tl" => tl = Some(m.next_value()?),
                        "tr" => tr = Some(m.next_value()?),
                        "br" => br = Some(m.next_value()?),
                        "bl" => bl = Some(m.next_value()?),
                        other => {
                            return Err(serde::de::Error::unknown_field(
                                other,
                                &["tl", "tr", "br", "bl"],
                            ));
                        }
                    }
                }
                Ok(Corners {
                    tl: tl.unwrap_or(0.0),
                    tr: tr.unwrap_or(0.0),
                    br: br.unwrap_or(0.0),
                    bl: bl.unwrap_or(0.0),
                })
            }
        }
        d.deserialize_any(V)
    }
}

impl Corners {
    pub const ZERO: Self = Self {
        tl: 0.0,
        tr: 0.0,
        br: 0.0,
        bl: 0.0,
    };
    pub const fn all(r: f32) -> Self {
        Self {
            tl: r,
            tr: r,
            br: r,
            bl: r,
        }
    }
    pub const fn new(tl: f32, tr: f32, br: f32, bl: f32) -> Self {
        Self { tl, tr, br, bl }
    }
    pub const fn scaled_by(&self, scale: f32) -> Self {
        Self {
            tl: self.tl * scale,
            tr: self.tr * scale,
            br: self.br * scale,
            bl: self.bl * scale,
        }
    }

    /// True when every corner is within UI epsilon of zero. Use this
    /// instead of `== Corners::ZERO` when the radii may have arrived via
    /// float math (DPR scaling, animation, theme deserialization) where
    /// exact equality is brittle.
    pub const fn approx_zero(&self) -> bool {
        use super::approx::approx_zero;
        approx_zero(self.tl) && approx_zero(self.tr) && approx_zero(self.br) && approx_zero(self.bl)
    }
}

impl<T: Num> From<T> for Corners {
    fn from(r: T) -> Self {
        Self::all(r.as_f32())
    }
}

impl From<Vec2> for Corners {
    fn from(v: Vec2) -> Self {
        Self {
            tl: v.x,
            tr: v.x,
            br: v.y,
            bl: v.y,
        }
    }
}

impl From<Size> for Corners {
    fn from(s: Size) -> Self {
        Self {
            tl: s.w,
            tr: s.w,
            br: s.h,
            bl: s.h,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap in a tiny struct so we can use TOML — top-level must be a table.
    fn ser(c: Corners) -> String {
        #[derive(serde::Serialize)]
        struct W {
            v: Corners,
        }
        toml::to_string(&W { v: c }).expect("serialize")
    }

    fn de(toml_str: &str) -> Corners {
        #[derive(serde::Deserialize)]
        struct W {
            v: Corners,
        }
        toml::from_str::<W>(toml_str).expect("parse").v
    }

    #[test]
    fn serialize_picks_compact_form_per_symmetry() {
        // "Matched pair" is exact-equality `tl==tr && br==bl`; near-matched
        // (`tl==br && tr==bl`) must NOT collapse — would lose data.
        let cases: &[(&str, Corners, &str)] = &[
            ("uniform_scalar", Corners::all(4.0), "v = 4.0"),
            (
                "matched_pairs_two_array",
                Corners {
                    tl: 4.0,
                    tr: 4.0,
                    br: 8.0,
                    bl: 8.0,
                },
                "v = [4.0, 8.0]",
            ),
            (
                "asymmetric_four_array",
                Corners {
                    tl: 1.0,
                    tr: 2.0,
                    br: 3.0,
                    bl: 4.0,
                },
                "v = [1.0, 2.0, 3.0, 4.0]",
            ),
            (
                "near_matched_does_not_collapse",
                Corners {
                    tl: 1.0,
                    tr: 2.0,
                    br: 1.0,
                    bl: 2.0,
                },
                "v = [1.0, 2.0, 1.0, 2.0]",
            ),
        ];
        for (label, c, want) in cases {
            assert_eq!(ser(*c).trim(), *want, "case: {label}");
        }
    }

    #[test]
    fn deserialize_accepts_scalar_array_and_integer_forms() {
        let cases: &[(&str, &str, Corners)] = &[
            ("scalar", "v = 4.0", Corners::all(4.0)),
            // Hand-written configs may use `radius = 4` rather than `4.0`.
            ("integer_scalar", "v = 4", Corners::all(4.0)),
            (
                "two_element_array",
                "v = [4.0, 8.0]",
                Corners {
                    tl: 4.0,
                    tr: 4.0,
                    br: 8.0,
                    bl: 8.0,
                },
            ),
            (
                "four_element_array",
                "v = [1.0, 2.0, 3.0, 4.0]",
                Corners {
                    tl: 1.0,
                    tr: 2.0,
                    br: 3.0,
                    bl: 4.0,
                },
            ),
            ("one_element_array_uniform", "v = [4.0]", Corners::all(4.0)),
        ];
        for (label, input, want) in cases {
            assert_eq!(de(input), *want, "case: {label}");
        }
    }

    #[test]
    fn deserialize_struct_form() {
        // Round-trips configs that hand-typed the struct shape (or
        // configs predating the array compaction).
        let toml_str = r#"
[v]
tl = 1.0
tr = 2.0
br = 3.0
bl = 4.0
"#;
        assert_eq!(
            de(toml_str),
            Corners {
                tl: 1.0,
                tr: 2.0,
                br: 3.0,
                bl: 4.0,
            }
        );
    }

    #[test]
    fn deserialize_struct_form_with_missing_fields_defaults_to_zero() {
        // Partial struct → omitted corners default to 0. Useful when
        // a config wants only some corners rounded.
        let toml_str = r#"
[v]
tl = 4.0
tr = 4.0
"#;
        assert_eq!(
            de(toml_str),
            Corners {
                tl: 4.0,
                tr: 4.0,
                br: 0.0,
                bl: 0.0,
            }
        );
    }

    #[test]
    fn deserialize_rejects_unknown_field() {
        #[derive(serde::Deserialize)]
        struct W {
            #[allow(dead_code)]
            v: Corners,
        }
        // Typo'd field names should fail loudly rather than silently
        // dropping the value.
        let result: Result<W, _> = toml::from_str(
            r#"
[v]
tl = 1.0
typo = 2.0
"#,
        );
        assert!(result.is_err(), "unknown field should be rejected");
    }

    /// Round-trip through TOML for each of the three serialize paths.
    #[test]
    fn serialize_then_parse_round_trips() {
        for c in [
            Corners::all(4.0),
            Corners {
                tl: 4.0,
                tr: 4.0,
                br: 8.0,
                bl: 8.0,
            },
            Corners {
                tl: 1.0,
                tr: 2.0,
                br: 3.0,
                bl: 4.0,
            },
        ] {
            let s = ser(c);
            assert_eq!(de(&s), c, "round-trip failed for {c:?} -> {s}");
        }
    }
}
