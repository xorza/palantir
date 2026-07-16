use crate::primitives::half_simd::F16x4;
use crate::primitives::lane_serde::{self, LaneCodec};
use crate::primitives::num::Num;
use crate::primitives::size::Size;
use glam::Vec2;

/// Per-corner radii, packed as four f16 lanes in a `u64` (8 bytes).
///
/// Lane layout (LE): `tl | tr | br | bl`. As `vec2<u32>` on the GPU
/// the first u32 carries `tl,tr` and the second `br,bl`; the shader
/// reconstructs `vec4<f32>` via two `unpack2x16float` calls.
///
/// Precision: lossless for integer radii up to 2048, ~0.25 px error at
/// 4096, +Inf above ~65504. Plenty of headroom for UI workloads.
///
/// Hash delegates to the packed `F16x4` representation — one `u64` write,
/// fed every frame into
/// `LayoutCore::hash` → `SubtreeRollups`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Corners(F16x4);

impl std::fmt::Debug for Corners {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [tl, tr, br, bl] = self.as_array();
        f.debug_struct("Corners")
            .field("tl", &tl)
            .field("tr", &tr)
            .field("br", &br)
            .field("bl", &bl)
            .finish()
    }
}

// Compact serde via the shared `lane_serde` codec:
// - all four equal → bare scalar `4.0`
// - tl=tr, br=bl   → 2-element array `[top, bottom]` (CSS-style shorthand)
// - otherwise      → 4-element array `[tl, tr, br, bl]`
// Deserialize also accepts the `{ tl, tr, br, bl }` table for
// hand-written configs.
impl LaneCodec for Corners {
    const FIELDS: &'static [&'static str] = &["tl", "tr", "br", "bl"];

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
        // tl==tr && br==bl → CSS-style [top, bottom].
        (l[0] == l[1] && l[2] == l[3]).then_some([l[0], l[2]])
    }
    #[inline]
    fn expand_two([top, bottom]: [f32; 2]) -> [f32; 4] {
        [top, top, bottom, bottom]
    }
}

impl serde::Serialize for Corners {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        lane_serde::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for Corners {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        lane_serde::deserialize(d)
    }
}

impl Corners {
    pub const ZERO: Self = Self(F16x4::ZERO);

    #[inline]
    pub fn all(r: f32) -> Self {
        Self(F16x4::from_lanes([r, r, r, r]))
    }

    #[inline]
    pub fn new(tl: f32, tr: f32, br: f32, bl: f32) -> Self {
        Self(F16x4::from_lanes([tl, tr, br, bl]))
    }

    /// Round the top edge only — `tl == tr == r`, `br == bl == 0`.
    #[inline]
    pub fn top(r: f32) -> Self {
        Self(F16x4::from_lanes([r, r, 0.0, 0.0]))
    }

    /// Round the bottom edge only.
    #[inline]
    pub fn bottom(r: f32) -> Self {
        Self(F16x4::from_lanes([0.0, 0.0, r, r]))
    }

    /// Round the left edge only.
    #[inline]
    pub fn left(r: f32) -> Self {
        Self(F16x4::from_lanes([r, 0.0, 0.0, r]))
    }

    /// Round the right edge only.
    #[inline]
    pub fn right(r: f32) -> Self {
        Self(F16x4::from_lanes([0.0, r, r, 0.0]))
    }

    /// CSS-style `[top, bottom]` shorthand.
    #[inline]
    pub fn top_bottom(top: f32, bottom: f32) -> Self {
        Self(F16x4::from_lanes([top, top, bottom, bottom]))
    }

    /// Round the `tl`/`br` diagonal pair (e.g. asymmetric chat bubble).
    #[inline]
    pub fn diag_main(r: f32) -> Self {
        Self(F16x4::from_lanes([r, 0.0, r, 0.0]))
    }

    /// Round the `tr`/`bl` diagonal pair.
    #[inline]
    pub fn diag_anti(r: f32) -> Self {
        Self(F16x4::from_lanes([0.0, r, 0.0, r]))
    }

    /// All four lanes unpacked at once. See `Spacing::as_array` for the
    /// SIMD rationale — same `half` slice path.
    #[inline]
    pub fn as_array(&self) -> [f32; 4] {
        self.0.lanes()
    }

    /// Inverse of [`Self::as_array`] — pack 4 runtime f32s into the
    /// lane array via the batched f32→f16 path (`fcvtn` on aarch64-fp16,
    /// `vcvtps2ph` on x86-f16c, scalar fallback). Use at hot sites that
    /// compute all 4 lanes at runtime. `Self::new` is the same path with
    /// the corners passed as separate args instead of an array.
    #[inline]
    pub fn from_array(v: [f32; 4]) -> Self {
        Self(F16x4::from_lanes(v))
    }

    #[inline]
    pub fn scaled_by(&self, scale: f32) -> Self {
        Self(self.0.scaled(scale))
    }

    /// True when every corner is within UI epsilon of zero. Routes
    /// through `approx::noop_f16_bits` so the bit-trick lives in one
    /// place — see that fn for the IEEE 754 rationale and NaN
    /// semantics.
    #[inline]
    pub const fn approx_zero(&self) -> bool {
        use crate::primitives::approx::noop_f16_bits;
        let [tl, tr, br, bl] = self.0.0;
        noop_f16_bits(tl) && noop_f16_bits(tr) && noop_f16_bits(br) && noop_f16_bits(bl)
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
    use crate::primitives::approx::EPS;
    use crate::primitives::corners::*;

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
        assert_eq!(c.as_array(), [1.0, 2.0, 3.0, 4.0]);
    }

    /// Documents the f16 precision contract: lossless for integer
    /// radii ≤ 2048, ~0.25 px quantization at 4096. A refactor that
    /// quietly switched storage (e.g. to Q8.8 fixed-point) would
    /// trip these.
    #[test]
    fn f16_precision_contract() {
        assert_eq!(Corners::all(2048.0).as_array()[0], 2048.0);
        let big = Corners::all(4096.0).as_array()[0];
        assert!(
            (big - 4096.0).abs() <= 0.25,
            "expected ≤0.25 px error at 4096, got {} -> {big}",
            (big - 4096.0).abs(),
        );
    }

    #[test]
    fn as_array_and_from_array_round_trip() {
        let original = Corners::new(1.0, 2.0, 3.0, 4.0);
        let arr = original.as_array();
        assert_eq!(arr, [1.0, 2.0, 3.0, 4.0]);
        let rebuilt = Corners::from_array(arr);
        assert_eq!(rebuilt, original);
    }

    #[test]
    fn scaled_by_multiplies_each_corner() {
        let c = Corners::new(2.0, 4.0, 6.0, 8.0).scaled_by(1.5);
        assert_eq!(c.as_array(), [3.0, 6.0, 9.0, 12.0]);
    }

    /// Pins the bit-trick path in `approx_zero`. ±0 lanes, sub-EPS,
    /// at-EPS, above-EPS, and NaN must all classify correctly.
    #[test]
    fn approx_zero_handles_edge_lane_patterns() {
        assert!(Corners::ZERO.approx_zero(), "all-zero bytes");
        assert!(Corners::all(0.0).approx_zero(), "+0.0 lanes");
        assert!(
            Corners::all(-0.0).approx_zero(),
            "-0.0 lanes (sign bit set)"
        );
        assert!(Corners::all(EPS * 0.5).approx_zero(), "sub-EPS positive",);
        assert!(
            !Corners::all(EPS * 10.0).approx_zero(),
            "10×EPS must NOT register as zero",
        );
        // One asymmetric lane above EPS — short-circuit must not
        // accept it just because the other three lanes are zero.
        assert!(
            !Corners::new(0.0, 0.0, 1.0, 0.0).approx_zero(),
            "single non-zero lane breaks zero contract",
        );
        // NaN bits land in the exponent region (≥ 0x7C00 absolute),
        // far above the EPS threshold — must classify as non-zero.
        assert!(
            !Corners::all(f32::NAN).approx_zero(),
            "NaN lanes are not zero"
        );
    }

    #[test]
    fn from_vec2_and_size_map_to_pairs() {
        use crate::primitives::size::Size;
        use glam::Vec2;
        assert_eq!(
            Corners::from(Vec2::new(3.0, 7.0)).as_array(),
            [3.0, 3.0, 7.0, 7.0],
            "Vec2 → (x,x,y,y)",
        );
        assert_eq!(
            Corners::from(Size::new(3.0, 7.0)).as_array(),
            [3.0, 3.0, 7.0, 7.0],
            "Size → (w,w,h,h)",
        );
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
