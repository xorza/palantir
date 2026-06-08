use crate::primitives::half_simd::F16x4;
use crate::primitives::lane_serde::LaneCodec;
use crate::primitives::num::Num;

/// Per-side spacing (padding / margin), packed as four f16 lanes in
/// `[u16; 4]` (8 bytes). Lane order: `left | top | right | bottom`.
///
/// Precision: lossless for integer values up to 2048, ~0.25 px error
/// at 4096. UI spacing never approaches the f16 ceiling.
///
/// Hash delegates to [`F16x4`] (one `u64` write) — `LayoutCore::hash`
/// folds this twice per node every frame (padding + margin), so the
/// single-write form matters.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Spacing(F16x4);

impl Spacing {
    /// Packed 8-byte form. Used by `LayoutCore::hash` to fold the
    /// padding + margin lanes into the parent hasher write.
    #[inline]
    pub(crate) fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

impl std::fmt::Debug for Spacing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [left, top, right, bottom] = self.as_array();
        f.debug_struct("Spacing")
            .field("left", &left)
            .field("top", &top)
            .field("right", &right)
            .field("bottom", &bottom)
            .finish()
    }
}

// Compact serde via the shared `lane_serde` codec:
// - all four equal         → bare scalar `4.0`
// - left=right, top=bottom → 2-element array `[horizontal, vertical]`
// - otherwise              → 4-element array `[left, top, right, bottom]`
// Deserialize also accepts the `{ left, top, right, bottom }` table for
// hand-written configs.
impl LaneCodec for Spacing {
    const FIELDS: &'static [&'static str] = &["left", "top", "right", "bottom"];

    #[inline]
    fn from_lane_array(l: [f32; 4]) -> Self {
        Self::new(l[0], l[1], l[2], l[3])
    }
    #[inline]
    fn to_lane_array(&self) -> [f32; 4] {
        self.as_array()
    }
    #[inline]
    fn two_form(l: [f32; 4]) -> Option<[f32; 2]> {
        // left==right && top==bottom → [horizontal, vertical].
        (l[0] == l[2] && l[1] == l[3]).then_some([l[0], l[1]])
    }
    #[inline]
    fn expand_two([horiz, vert]: [f32; 2]) -> [f32; 4] {
        [horiz, vert, horiz, vert]
    }
}

impl serde::Serialize for Spacing {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        crate::primitives::lane_serde::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for Spacing {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::primitives::lane_serde::deserialize(d)
    }
}

impl Spacing {
    pub const ZERO: Self = Self(F16x4::ZERO);

    #[inline]
    pub fn all(v: f32) -> Self {
        Self(F16x4::from_lanes([v, v, v, v]))
    }

    #[inline]
    pub fn xy(x: f32, y: f32) -> Self {
        Self(F16x4::from_lanes([x, y, x, y]))
    }

    #[inline]
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self(F16x4::from_lanes([left, top, right, bottom]))
    }

    /// All four lanes unpacked at once. Routes through `half`'s
    /// platform-specific batched f16→f32 path (single `fcvtl` on
    /// aarch64-fp16, `vcvtph2ps` on x86-f16c, scalar fallback elsewhere).
    /// Use at hot sites that read 3+ lanes to amortize feature dispatch.
    #[inline]
    pub fn as_array(&self) -> [f32; 4] {
        self.0.lanes()
    }

    /// Inverse of [`Self::as_array`] — batched runtime f32→f16 pack.
    /// See `Corners::from_array` for the SIMD rationale.
    #[inline]
    pub fn from_array(v: [f32; 4]) -> Self {
        Self(F16x4::from_lanes(v))
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
    /// Both totals in a single SIMD unpack. Use when both axes are
    /// needed; otherwise prefer `horiz()` / `vert()`.
    #[inline]
    pub fn sums(&self) -> Sums {
        let [l, t, r, b] = self.as_array();
        Sums {
            horiz: l + r,
            vert: t + b,
        }
    }
}

/// Both axis totals from one [`Spacing`], unpacked together — `horiz =
/// left + right`, `vert = top + bottom`.
#[derive(Clone, Copy, Debug)]
pub struct Sums {
    pub horiz: f32,
    pub vert: f32,
}

impl std::ops::Add for Spacing {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        let [al, at, ar, ab] = self.as_array();
        let [bl, bt, br, bb] = rhs.as_array();
        Self::from_array([al + bl, at + bt, ar + br, ab + bb])
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
    use crate::primitives::spacing::*;

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
        assert_eq!(s.as_array(), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(s.horiz(), 4.0);
        assert_eq!(s.vert(), 6.0);
    }

    /// Documents the f16 precision contract: lossless for integer
    /// values ≤ 2048, ~0.25 px quantization at 4096.
    #[test]
    fn f16_precision_contract() {
        assert_eq!(Spacing::all(2048.0).as_array()[0], 2048.0);
        let big = Spacing::all(4096.0).as_array()[0];
        assert!(
            (big - 4096.0).abs() <= 0.25,
            "expected ≤0.25 px error at 4096, got {big}",
        );
    }

    #[test]
    fn as_array_and_from_array_round_trip() {
        let original = Spacing::new(1.0, 2.0, 3.0, 4.0);
        let arr = original.as_array();
        assert_eq!(arr, [1.0, 2.0, 3.0, 4.0]);
        let rebuilt = Spacing::from_array(arr);
        assert_eq!(rebuilt, original);
    }

    #[test]
    fn xy_ctor_repeats_axes() {
        let s = Spacing::xy(3.0, 7.0);
        assert_eq!(s.as_array(), [3.0, 7.0, 3.0, 7.0]);
        assert_eq!(s.horiz(), 6.0);
        assert_eq!(s.vert(), 14.0);
    }

    /// Tuple `From` impls — easy place to swap component order during
    /// a refactor. Pin both forms.
    #[test]
    fn from_tuple_preserves_component_order() {
        let xy: Spacing = (3, 7).into();
        assert_eq!(xy.as_array(), [3.0, 7.0, 3.0, 7.0]);
        let ltrb: Spacing = (1, 2, 3, 4).into();
        assert_eq!(ltrb.as_array(), [1.0, 2.0, 3.0, 4.0]);
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
        assert_eq!(c.as_array(), [11.0, 22.0, 33.0, 44.0]);
    }
}
