use super::num::Num;
use super::size::Size;
use glam::Vec2;
use half::f16;

/// Per-corner radii, packed as four f16 lanes in a `u64` (8 bytes).
///
/// Lane layout (LE): `tl | tr | br | bl`. As `vec2<u32>` on the GPU
/// the first u32 carries `tl,tr` and the second `br,bl`; the shader
/// reconstructs `vec4<f32>` via two `unpack2x16float` calls.
///
/// Precision: lossless for integer radii up to 2048, ~0.25 px error at
/// 4096, +Inf above ~65504. Plenty of headroom for UI workloads.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Corners([u16; 4]);

impl std::fmt::Debug for Corners {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Corners")
            .field("tl", &self.tl())
            .field("tr", &self.tr())
            .field("br", &self.br())
            .field("bl", &self.bl())
            .finish()
    }
}

const fn pack(tl: f32, tr: f32, br: f32, bl: f32) -> [u16; 4] {
    [
        f16::from_f32_const(tl).to_bits(),
        f16::from_f32_const(tr).to_bits(),
        f16::from_f32_const(br).to_bits(),
        f16::from_f32_const(bl).to_bits(),
    ]
}

#[inline]
fn lane(bits: u16) -> f32 {
    f16::from_bits(bits).to_f32()
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
        let (tl, tr, br, bl) = (self.tl(), self.tr(), self.br(), self.bl());
        if tl == tr && tr == br && br == bl {
            return s.serialize_f32(tl);
        }
        if tl == tr && br == bl {
            let mut seq = s.serialize_seq(Some(2))?;
            seq.serialize_element(&tl)?;
            seq.serialize_element(&br)?;
            return seq.end();
        }
        let mut seq = s.serialize_seq(Some(4))?;
        seq.serialize_element(&tl)?;
        seq.serialize_element(&tr)?;
        seq.serialize_element(&br)?;
        seq.serialize_element(&bl)?;
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
                    return Ok(Corners::new(v0, v0, v1, v1));
                };
                let v3: f32 = a
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;
                Ok(Corners::new(v0, v1, v2, v3))
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
                Ok(Corners::new(
                    tl.unwrap_or(0.0),
                    tr.unwrap_or(0.0),
                    br.unwrap_or(0.0),
                    bl.unwrap_or(0.0),
                ))
            }
        }
        d.deserialize_any(V)
    }
}

impl Corners {
    pub const ZERO: Self = Self([0; 4]);

    pub const fn all(r: f32) -> Self {
        Self(pack(r, r, r, r))
    }

    pub const fn new(tl: f32, tr: f32, br: f32, bl: f32) -> Self {
        Self(pack(tl, tr, br, bl))
    }

    /// Round the top edge only — `tl == tr == r`, `br == bl == 0`.
    pub const fn top(r: f32) -> Self {
        Self(pack(r, r, 0.0, 0.0))
    }

    /// Round the bottom edge only.
    pub const fn bottom(r: f32) -> Self {
        Self(pack(0.0, 0.0, r, r))
    }

    /// Round the left edge only.
    pub const fn left(r: f32) -> Self {
        Self(pack(r, 0.0, 0.0, r))
    }

    /// Round the right edge only.
    pub const fn right(r: f32) -> Self {
        Self(pack(0.0, r, r, 0.0))
    }

    /// CSS-style `[top, bottom]` shorthand.
    pub const fn top_bottom(top: f32, bottom: f32) -> Self {
        Self(pack(top, top, bottom, bottom))
    }

    /// Round the `tl`/`br` diagonal pair (e.g. asymmetric chat bubble).
    pub const fn diag_main(r: f32) -> Self {
        Self(pack(r, 0.0, r, 0.0))
    }

    /// Round the `tr`/`bl` diagonal pair.
    pub const fn diag_anti(r: f32) -> Self {
        Self(pack(0.0, r, 0.0, r))
    }

    #[inline]
    pub fn tl(&self) -> f32 {
        lane(self.0[0])
    }
    #[inline]
    pub fn tr(&self) -> f32 {
        lane(self.0[1])
    }
    #[inline]
    pub fn br(&self) -> f32 {
        lane(self.0[2])
    }
    #[inline]
    pub fn bl(&self) -> f32 {
        lane(self.0[3])
    }

    /// All four lanes unpacked at once. See `Spacing::as_array` for the
    /// SIMD rationale — same `half` slice path.
    #[inline]
    pub fn as_array(&self) -> [f32; 4] {
        use half::slice::HalfFloatSliceExt;
        let arr: &[half::f16; 4] = bytemuck::cast_ref(&self.0);
        let mut out = [0.0f32; 4];
        arr.as_slice().convert_to_f32_slice(&mut out);
        out
    }

    #[inline]
    pub fn scaled_by(&self, scale: f32) -> Self {
        let [tl, tr, br, bl] = self.as_array();
        Self::new(tl * scale, tr * scale, br * scale, bl * scale)
    }

    /// True when every corner is within UI epsilon of zero. Compares
    /// the f16 lanes' absolute-value bit patterns against a precomputed
    /// `EPS` threshold — no f16→f32 conversion, no SIMD. Correct for
    /// ±0, subnormals, NaN (NaN's masked exponent is `0x7C00`, far
    /// above the threshold so it returns false, matching the f32
    /// semantics of `approx_zero`).
    #[inline]
    pub fn approx_zero(&self) -> bool {
        const EPS_BITS: u16 = half::f16::from_f32_const(super::approx::EPS).to_bits();
        const ABS_MASK: u16 = 0x7FFF;
        (self.0[0] & ABS_MASK) <= EPS_BITS
            && (self.0[1] & ABS_MASK) <= EPS_BITS
            && (self.0[2] & ABS_MASK) <= EPS_BITS
            && (self.0[3] & ABS_MASK) <= EPS_BITS
    }
}

impl<T: Num> From<T> for Corners {
    fn from(r: T) -> Self {
        Self::all(r.as_f32())
    }
}

impl From<Vec2> for Corners {
    fn from(v: Vec2) -> Self {
        Self::new(v.x, v.x, v.y, v.y)
    }
}

impl From<Size> for Corners {
    fn from(s: Size) -> Self {
        Self::new(s.w, s.w, s.h, s.h)
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
    fn struct_is_eight_bytes() {
        assert_eq!(std::mem::size_of::<Corners>(), 8);
        // align 2 (not 8) so embedding inside `Quad` doesn't bump
        // Quad's alignment above 4 and introduce trailing pad bytes
        // that break the `Pod` no-padding contract.
    }

    #[test]
    fn lanes_round_trip_integer_values_exactly() {
        let c = Corners::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(c.tl(), 1.0);
        assert_eq!(c.tr(), 2.0);
        assert_eq!(c.br(), 3.0);
        assert_eq!(c.bl(), 4.0);
    }

    #[test]
    fn convenience_ctors() {
        assert_eq!(Corners::top(4.0).as_array(), [4.0, 4.0, 0.0, 0.0]);
        assert_eq!(Corners::bottom(4.0).as_array(), [0.0, 0.0, 4.0, 4.0]);
        assert_eq!(Corners::left(4.0).as_array(), [4.0, 0.0, 0.0, 4.0]);
        assert_eq!(Corners::right(4.0).as_array(), [0.0, 4.0, 4.0, 0.0]);
        assert_eq!(
            Corners::top_bottom(2.0, 8.0).as_array(),
            [2.0, 2.0, 8.0, 8.0]
        );
        assert_eq!(Corners::diag_main(5.0).as_array(), [5.0, 0.0, 5.0, 0.0]);
        assert_eq!(Corners::diag_anti(5.0).as_array(), [0.0, 5.0, 0.0, 5.0]);
    }

    #[test]
    fn serialize_picks_compact_form_per_symmetry() {
        let cases: &[(&str, Corners, &str)] = &[
            ("uniform_scalar", Corners::all(4.0), "v = 4.0"),
            (
                "matched_pairs_two_array",
                Corners::new(4.0, 4.0, 8.0, 8.0),
                "v = [4.0, 8.0]",
            ),
            (
                "asymmetric_four_array",
                Corners::new(1.0, 2.0, 3.0, 4.0),
                "v = [1.0, 2.0, 3.0, 4.0]",
            ),
            (
                "near_matched_does_not_collapse",
                Corners::new(1.0, 2.0, 1.0, 2.0),
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
            ("integer_scalar", "v = 4", Corners::all(4.0)),
            (
                "two_element_array",
                "v = [4.0, 8.0]",
                Corners::new(4.0, 4.0, 8.0, 8.0),
            ),
            (
                "four_element_array",
                "v = [1.0, 2.0, 3.0, 4.0]",
                Corners::new(1.0, 2.0, 3.0, 4.0),
            ),
            ("one_element_array_uniform", "v = [4.0]", Corners::all(4.0)),
        ];
        for (label, input, want) in cases {
            assert_eq!(de(input), *want, "case: {label}");
        }
    }

    #[test]
    fn deserialize_struct_form() {
        let toml_str = r#"
[v]
tl = 1.0
tr = 2.0
br = 3.0
bl = 4.0
"#;
        assert_eq!(de(toml_str), Corners::new(1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn deserialize_struct_form_with_missing_fields_defaults_to_zero() {
        let toml_str = r#"
[v]
tl = 4.0
tr = 4.0
"#;
        assert_eq!(de(toml_str), Corners::new(4.0, 4.0, 0.0, 0.0));
    }

    #[test]
    fn deserialize_rejects_unknown_field() {
        #[derive(serde::Deserialize)]
        struct W {
            #[allow(dead_code)]
            v: Corners,
        }
        let result: Result<W, _> = toml::from_str(
            r#"
[v]
tl = 1.0
typo = 2.0
"#,
        );
        assert!(result.is_err(), "unknown field should be rejected");
    }

    #[test]
    fn serialize_then_parse_round_trips() {
        for c in [
            Corners::all(4.0),
            Corners::new(4.0, 4.0, 8.0, 8.0),
            Corners::new(1.0, 2.0, 3.0, 4.0),
        ] {
            let s = ser(c);
            assert_eq!(de(&s), c, "round-trip failed for {c:?} -> {s}");
        }
    }
}
