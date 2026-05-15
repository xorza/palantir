use crate::animation::animatable::Animatable;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::num::canon_bits;
use glam::Vec2;
use tinyvec::ArrayVec;

/// Hard cap on stops in a single gradient. 8 covers >99% of UI use
/// (2-3 stops dominate, multi-stop bars rarely exceed 5). Hard-asserted
/// in `LinearGradient::new` — exceeding the cap is a caller bug, not a
/// silent truncation.
pub const MAX_STOPS: usize = 8;

/// GPU-wire form of a gradient's axis: four f16 lanes (`[u16; 4]`,
/// 8 B). Variant-dependent layout — `[dir_x, dir_y, t0, t1]` for
/// linear, `[cx, cy, rx, ry]` for radial, `[cx, cy, start_angle, _]`
/// for conic, `[offset.x, offset.y, σ, axis_w]` for shadow. Mirrors
/// `Corners`'s u64 lane scheme — the WGSL vertex attribute is
/// `vec2<u32>` and the shader unpacks via two `unpack2x16float`
/// calls into the same `vec4<f32>` the fragment shader sees.
///
/// f16 precision (~3 decimal digits) is plenty for unit direction
/// vectors and the 0..1 parametric range; sub-pixel error envelope
/// up to ~2048 px, then degrading like `Corners`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FillAxis(pub [u16; 4]);

impl FillAxis {
    /// All-zero axis used for solid quads. The shader ignores it when
    /// `FillKind::is_solid()`, so the value doesn't matter — keep it
    /// zeroed so Pod-byte cache keys are deterministic for solid
    /// quads.
    pub const ZERO: Self = Self([0; 4]);

    /// Build from four runtime f32 lanes via the batched f16 slice
    /// path. Single SIMD instruction on F16C/fp16 targets.
    #[inline]
    pub fn from_lanes(a: f32, b: f32, c: f32, d: f32) -> Self {
        use half::slice::HalfFloatSliceExt;
        let src = [a, b, c, d];
        let mut out = [half::f16::ZERO; 4];
        out.as_mut_slice().convert_from_f32_slice(&src);
        Self(bytemuck::cast(out))
    }

    /// All four lanes unpacked at once via the batched slice path —
    /// matches `Corners::as_array`.
    #[inline]
    pub fn lanes(self) -> [f32; 4] {
        use half::slice::HalfFloatSliceExt;
        let arr: &[half::f16; 4] = bytemuck::cast_ref(&self.0);
        let mut out = [0.0f32; 4];
        arr.as_slice().convert_to_f32_slice(&mut out);
        out
    }

    #[inline]
    pub fn t0(self) -> f32 {
        half::f16::from_bits(self.0[2]).to_f32()
    }

    /// Per-lane f32 setter helper for the composer's
    /// `current_transform.scale` multiply path. Re-quantizes via the
    /// scalar f16 round-trip.
    #[inline]
    pub fn scaled(self, s: f32) -> Self {
        let [a, b, c, d] = self.lanes();
        Self::from_lanes(a * s, b * s, c * s, d * s)
    }
}

/// One colour stop in a gradient. `offset_u8` is the 0..1 parametric
/// position quantized to 8 bits (256 levels — finer than the LUT it
/// bakes into). `color` is 8-bit sRGB. Total 5 B / stop, align 1, so
/// `ArrayVec<[Stop; 8]>` is 40 B inline vs. 64 B with f32 offsets.
/// Stops are storage-only (never animated; snap on morph), feed a u8
/// LUT, and out-of-range positions clamp at construction — 8-bit
/// precision is sufficient and saves ~24 B per gradient.
#[derive(Copy, Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stop {
    pub offset_u8: u8,
    pub color: ColorU8,
}

impl Stop {
    /// Construct a stop. `offset` is clamped to 0..=1 and quantized to
    /// u8 (round-to-nearest); out-of-range values clamp at construction
    /// rather than at bake time so the stored value matches what
    /// authors expect to round-trip.
    #[inline]
    pub fn new(offset: f32, color: impl Into<ColorU8>) -> Self {
        let q = (offset.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Self {
            offset_u8: q,
            color: color.into(),
        }
    }

    /// Decode the stored quantized position back to a 0..1 f32 for
    /// consumers (atlas bake, axis calc) that interpolate in float.
    #[inline]
    pub fn offset(self) -> f32 {
        self.offset_u8 as f32 / 255.0
    }
}

/// How the gradient repeats outside the 0..1 parametric range.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Spread {
    /// Clamp to nearest edge stop. CSS default.
    #[default]
    Pad = 0,
    /// Tile 0..1 across the surface.
    Repeat = 1,
    /// Tile mirrored.
    Reflect = 2,
}

/// Colour space the interpolation runs in. Affects the perceived
/// transition; doesn't change the stop colours themselves.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Interp {
    /// Perceptually uniform; matches CSS Color 4 default. Avoids the
    /// muddy midpoint of complementary-colour pairs (red↔green,
    /// blue↔orange).
    #[default]
    Oklab,
    /// Linear-RGB interpolation. Cheapest; what most rendering engines
    /// do by default. Visible midpoint dip on saturated complementary
    /// pairs.
    Linear,
}

/// Linear gradient — paints colour along an axis at `angle` radians
/// (0 = →, π/2 = ↓). Object-space: gradient spans the brush owner's
/// bounding rect end-to-end at the given angle.
///
/// Stops live inline via `ArrayVec<[Stop; MAX_STOPS]>` so a
/// `LinearGradient` value is heap-free and `Copy`. Total size is ~80 B
/// (4 B angle + 64 B stops + 1 B spread + 1 B interp + pad).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LinearGradient {
    pub angle: f32,
    pub stops: ArrayVec<[Stop; MAX_STOPS]>,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for LinearGradient {
    /// Hand-written: f32 fields (`angle`, per-stop `offset`) need
    /// canonical bit encoding via `canon_bits` so `-0.0` / `+0.0` and
    /// NaN bit patterns don't fragment cache keys. Used by command-
    /// buffer dedup; the atlas hashes `(stops, interp)` separately
    /// (variant-agnostic) in `gradient_atlas::hash_stops`.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Pack `(angle, spread, interp, len)` into one u64 — angle's
        // canonical bits go in the high lane, the three u8 tags
        // (spread/interp/len, total 24 bits) ride in the low lane.
        state.write_u64(
            ((canon_bits(self.angle) as u64) << 32)
                | ((self.spread as u64) << 16)
                | ((self.interp as u64) << 8)
                | (self.stops.len() as u64),
        );
        for s in self.stops.iter() {
            state.write_u64(((s.color.to_u32() as u64) << 32) | (s.offset_u8 as u64));
        }
    }
}

impl LinearGradient {
    /// General constructor. Asserts 2..=MAX_STOPS stops.
    pub fn new(angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            angle,
            stops: collect_stops::<{ MAX_STOPS }>(stops, "LinearGradient"),
            spread: Spread::default(),
            interp: Interp::default(),
        }
    }

    /// 2-stop shorthand — `c0` at offset 0, `c1` at offset 1. Covers
    /// the dominant UI-gradient pattern (panel chrome, button
    /// surfaces, headers).
    pub fn two_stop(angle: f32, c0: impl Into<ColorU8>, c1: impl Into<ColorU8>) -> Self {
        Self::new(angle, [Stop::new(0.0, c0), Stop::new(1.0, c1)])
    }

    /// 3-stop shorthand — `c0` at 0, `c1` at 0.5, `c2` at 1.
    pub fn three_stop(
        angle: f32,
        c0: impl Into<ColorU8>,
        c1: impl Into<ColorU8>,
        c2: impl Into<ColorU8>,
    ) -> Self {
        Self::new(
            angle,
            [Stop::new(0.0, c0), Stop::new(0.5, c1), Stop::new(1.0, c2)],
        )
    }

    pub fn with_spread(mut self, spread: Spread) -> Self {
        self.spread = spread;
        self
    }

    pub fn with_interp(mut self, interp: Interp) -> Self {
        self.interp = interp;
        self
    }

    /// Paints nothing visible when every stop is transparent.
    pub fn is_noop(&self) -> bool {
        self.stops.iter().all(|s| s.color.is_noop())
    }

    /// Gradient axis for the shader. `dir = (cos(angle), sin(angle))`;
    /// the shader projects each fragment's 0..1 object-local position
    /// onto `dir`, then maps the dot product through `(t0, t1)` to
    /// the LUT row.
    ///
    /// Slice 2 always emits `(t0, t1) = (0, 1)` and the raw
    /// `(cos, sin)` axis; diagonal gradients project to a sub-1.0
    /// range and rely on `Spread::Pad` to clamp. CSS-style
    /// corner-to-corner scaling is a slice 2.5 polish task.
    pub fn axis(&self) -> FillAxis {
        let (sin, cos) = self.angle.sin_cos();
        FillAxis::from_lanes(cos, sin, 0.0, 1.0)
    }
}

/// Radial gradient — paints colour outward from `center` along the
/// elliptical radius `radius`. Object-space: both `center` and `radius`
/// are in 0..1 coordinates (origin top-left, (1,1) bottom-right of the
/// brush owner). The shader projects each fragment to
/// `t = length((local01 - center) / radius)`, applies `Spread`, and
/// samples the LUT.
///
/// Per-variant `Interp` default: `Oklab` — radial fills are usually
/// soft glows where perceptual smoothness matters most.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RadialGradient {
    pub center: Vec2,
    pub radius: Vec2,
    pub stops: ArrayVec<[Stop; MAX_STOPS]>,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for RadialGradient {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            ((canon_bits(self.center.x) as u64) << 32) | (canon_bits(self.center.y) as u64),
        );
        state.write_u64(
            ((canon_bits(self.radius.x) as u64) << 32) | (canon_bits(self.radius.y) as u64),
        );
        state.write_u64(
            ((self.spread as u64) << 16) | ((self.interp as u64) << 8) | (self.stops.len() as u64),
        );
        for s in self.stops.iter() {
            state.write_u64(((s.color.to_u32() as u64) << 32) | (s.offset_u8 as u64));
        }
    }
}

impl RadialGradient {
    pub fn new(center: Vec2, radius: Vec2, stops: impl IntoIterator<Item = Stop>) -> Self {
        let stops = collect_stops::<{ MAX_STOPS }>(stops, "RadialGradient");
        Self {
            center,
            radius,
            stops,
            spread: Spread::default(),
            interp: Interp::Oklab,
        }
    }

    /// 2-stop centred shorthand — `center = (0.5, 0.5)`,
    /// `radius = (0.5, 0.5)` (covers the bounding circle inscribed in
    /// the unit square). `c0` at offset 0 (centre), `c1` at offset 1
    /// (edge).
    pub fn two_stop_centered(c0: impl Into<ColorU8>, c1: impl Into<ColorU8>) -> Self {
        Self::new(
            Vec2::splat(0.5),
            Vec2::splat(0.5),
            [Stop::new(0.0, c0), Stop::new(1.0, c1)],
        )
    }

    pub fn with_spread(mut self, spread: Spread) -> Self {
        self.spread = spread;
        self
    }

    pub fn with_interp(mut self, interp: Interp) -> Self {
        self.interp = interp;
        self
    }

    pub fn is_noop(&self) -> bool {
        self.stops.iter().all(|s| s.color.is_noop())
    }

    /// Pack `(center, radius)` into a `FillAxis` wire slot. The shader
    /// reads it as `(cx, cy, rx, ry)` for the radial branch.
    pub fn axis(&self) -> FillAxis {
        FillAxis::from_lanes(self.center.x, self.center.y, self.radius.x, self.radius.y)
    }
}

/// Conic (sweep) gradient — paints colour by sweeping the parametric
/// axis 0..1 around `center` starting at `start_angle` radians,
/// counter-clockwise. Object-space `center` is in 0..1 coordinates.
/// The shader projects each fragment to
/// `t = fract((atan2(dy, dx) - start_angle) / TAU + 1.0)`, applies
/// `Spread`, samples the LUT.
///
/// Per-variant `Interp` default: `Linear`. Conic gradients commonly
/// implement colour-wheel / hue-rotation visuals where straight
/// linear-RGB interpolation gives the most predictable hue sweep;
/// Oklab can shift the perceived hue at the midpoint. (A future
/// `Oklch{hue}` interp would be the truly right default — see
/// `docs/roadmap/brushes.md`.)
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConicGradient {
    pub center: Vec2,
    pub start_angle: f32,
    pub stops: ArrayVec<[Stop; MAX_STOPS]>,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for ConicGradient {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            ((canon_bits(self.center.x) as u64) << 32) | (canon_bits(self.center.y) as u64),
        );
        // Pack `(start_angle, spread, interp, len)` into one u64 like
        // `LinearGradient` — the f32 sits in the high lane, three u8
        // tags in the low lane.
        state.write_u64(
            ((canon_bits(self.start_angle) as u64) << 32)
                | ((self.spread as u64) << 16)
                | ((self.interp as u64) << 8)
                | (self.stops.len() as u64),
        );
        for s in self.stops.iter() {
            state.write_u64(((s.color.to_u32() as u64) << 32) | (s.offset_u8 as u64));
        }
    }
}

impl ConicGradient {
    pub fn new(center: Vec2, start_angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        let stops = collect_stops::<{ MAX_STOPS }>(stops, "ConicGradient");
        Self {
            center,
            start_angle,
            stops,
            spread: Spread::default(),
            interp: Interp::Linear,
        }
    }

    /// Centred shorthand — `center = (0.5, 0.5)`, starts at angle 0
    /// (positive x-axis, sweeping CCW). 2 stops at offsets 0/1.
    pub fn two_stop_centered(c0: impl Into<ColorU8>, c1: impl Into<ColorU8>) -> Self {
        Self::new(
            Vec2::splat(0.5),
            0.0,
            [Stop::new(0.0, c0), Stop::new(1.0, c1)],
        )
    }

    pub fn with_spread(mut self, spread: Spread) -> Self {
        self.spread = spread;
        self
    }

    pub fn with_interp(mut self, interp: Interp) -> Self {
        self.interp = interp;
        self
    }

    pub fn is_noop(&self) -> bool {
        self.stops.iter().all(|s| s.color.is_noop())
    }

    /// Pack `(center, start_angle)` into a `FillAxis` wire slot. The
    /// shader reads it as `(cx, cy, start_angle, _)` for the conic
    /// branch; `t1` is unused.
    pub fn axis(&self) -> FillAxis {
        FillAxis::from_lanes(self.center.x, self.center.y, self.start_angle, 0.0)
    }
}

/// Shared 2..=MAX_STOPS validation used by every gradient constructor.
fn collect_stops<const N: usize>(
    stops: impl IntoIterator<Item = Stop>,
    ty: &'static str,
) -> ArrayVec<[Stop; MAX_STOPS]> {
    let mut sv: ArrayVec<[Stop; MAX_STOPS]> = ArrayVec::new();
    for s in stops {
        assert!(
            sv.len() < MAX_STOPS,
            "{ty}: stop count exceeds MAX_STOPS = {MAX_STOPS}",
        );
        sv.push(s);
    }
    assert!(
        sv.len() >= 2,
        "{ty} requires at least 2 stops, got {}",
        sv.len(),
    );
    sv
}

/// Paint source for fills and strokes.
///
/// `Solid(Color)` is the hot 99% path — 16 B inline, animation-lerpable.
/// `Linear`/`Radial`/`Conic` carry their geometry inline (~80 B);
/// gradient morph animations snap across variants and across distinct
/// gradients of the same variant (see `docs/roadmap/brushes.md` "Future
/// work: gradient morph animation"). Stroke-with-gradient is still
/// solid-only; lowering sites call `as_solid().expect(...)` for stroke.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Brush {
    Solid(Color),
    Linear(LinearGradient),
    Radial(RadialGradient),
    Conic(ConicGradient),
}

impl Brush {
    pub const TRANSPARENT: Self = Self::Solid(Color::TRANSPARENT);

    /// Paints nothing visible.
    #[inline]
    pub fn is_noop(&self) -> bool {
        match self {
            Brush::Solid(c) => c.is_noop(),
            Brush::Linear(g) => g.is_noop(),
            Brush::Radial(g) => g.is_noop(),
            Brush::Conic(g) => g.is_noop(),
        }
    }

    /// Extracts the underlying `Color` for the solid fast path. Returns
    /// `None` for gradient variants; downstream sites that don't yet
    /// support gradient paint (currently: stroke) `.expect()` with a
    /// TODO message.
    #[inline]
    pub const fn as_solid(self) -> Option<Color> {
        match self {
            Brush::Solid(c) => Some(c),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => None,
        }
    }

    /// Unwrap to the solid color, panicking on gradient variants with the
    /// shared "not yet implemented" message. Centralizes the lowering-side
    /// gradient-not-supported assert; remove this method when slice 2 lands.
    #[inline]
    #[track_caller]
    pub fn expect_solid(self) -> Color {
        self.as_solid().expect(
            "gradient brush rendering not yet implemented; see docs/roadmap/brushes.md slice 2",
        )
    }
}

impl Default for Brush {
    #[inline]
    fn default() -> Self {
        Brush::TRANSPARENT
    }
}

impl From<Color> for Brush {
    #[inline]
    fn from(c: Color) -> Self {
        Brush::Solid(c)
    }
}

impl std::hash::Hash for Brush {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Brush::Solid(c) => {
                state.write_u8(0);
                c.hash(state);
            }
            Brush::Linear(g) => {
                state.write_u8(1);
                g.hash(state);
            }
            Brush::Radial(g) => {
                state.write_u8(2);
                g.hash(state);
            }
            Brush::Conic(g) => {
                state.write_u8(3);
                g.hash(state);
            }
        }
    }
}

impl Animatable for Brush {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        match (a, b) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(Color::lerp(x, y, t)),
            // Gradient morphs snap; see docs/roadmap/brushes.md
            // "Future work: gradient morph animation."
            _ => {
                if t >= 1.0 {
                    b
                } else {
                    a
                }
            }
        }
    }
    #[inline]
    fn sub(self, other: Self) -> Self {
        match (self, other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.sub(y)),
            _ => self,
        }
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        match (self, other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.add(y)),
            _ => self,
        }
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        match self {
            Brush::Solid(c) => Brush::Solid(c.scale(k)),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => self,
        }
    }
    #[inline]
    fn magnitude_squared(self) -> f32 {
        match self {
            Brush::Solid(c) => c.magnitude_squared(),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => 0.0,
        }
    }
    #[inline]
    fn zero() -> Self {
        Brush::Solid(Color::zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn h(b: Brush) -> u64 {
        let mut s = DefaultHasher::new();
        b.hash(&mut s);
        s.finish()
    }

    /// `LinearGradient::Hash` feeds `GradientCpuAtlas::register`'s
    /// content-hashed row addressing — `±0.0` and NaN bit-pattern variants
    /// must collapse so visually-identical gradients reuse one atlas row.
    #[test]
    fn linear_gradient_canon_bits_collapses_equivalent_f32_patterns() {
        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        assert!(nan_a.is_nan() && nan_b.is_nan());
        let cases: &[(&str, Brush, Brush)] = &[
            (
                "angle_neg_zero_eq_pos_zero",
                Brush::Linear(LinearGradient::two_stop(0.0, Color::BLACK, Color::WHITE)),
                Brush::Linear(LinearGradient::two_stop(-0.0, Color::BLACK, Color::WHITE)),
            ),
            (
                "angle_nan_bit_patterns_collapse",
                Brush::Linear(LinearGradient::two_stop(nan_a, Color::BLACK, Color::WHITE)),
                Brush::Linear(LinearGradient::two_stop(nan_b, Color::BLACK, Color::WHITE)),
            ),
            (
                "stop_offset_neg_zero_eq_pos_zero",
                Brush::Linear(LinearGradient::new(
                    0.0,
                    [Stop::new(0.0, Color::BLACK), Stop::new(1.0, Color::WHITE)],
                )),
                Brush::Linear(LinearGradient::new(
                    0.0,
                    [Stop::new(-0.0, Color::BLACK), Stop::new(1.0, Color::WHITE)],
                )),
            ),
        ];
        for (label, x, y) in cases {
            assert_eq!(h(*x), h(*y), "case: {label}");
        }
    }

    #[test]
    fn from_color_round_trip() {
        let c = Color::WHITE;
        let b: Brush = c.into();
        assert_eq!(b, Brush::Solid(c));
        assert_eq!(b.as_solid(), Some(c));
    }

    #[test]
    fn solid_solid_animatable_lerp_matches_color() {
        let a = Color::BLACK;
        let b = Color::WHITE;
        let mid_color = Color::lerp(a, b, 0.5);
        let mid_brush = Brush::lerp(Brush::Solid(a), Brush::Solid(b), 0.5);
        assert_eq!(mid_brush, Brush::Solid(mid_color));
    }

    #[test]
    fn solid_is_noop_iff_color_is_noop() {
        assert!(Brush::Solid(Color::TRANSPARENT).is_noop());
        assert!(!Brush::Solid(Color::BLACK).is_noop());
    }

    /// `LinearGradient` is inline-stored on every `Brush::Linear`, so
    /// its size sets the floor for `Brush`, `Background.fill`,
    /// `Stroke.brush`, and every `Shape::*` variant carrying a brush.
    /// Pin the size so any silent footprint regression (added field,
    /// stop-cap bump) trips a test rather than diffusing through the
    /// codebase. The exact number below is a function of `MAX_STOPS = 8`
    /// + `repr(C)` field layout; recompute when those change.
    #[test]
    fn linear_gradient_size_is_compact() {
        // 4 (angle) + ArrayVec<[Stop; 8]> with Stop = 5 B (1 offset_u8 + 4 ColorU8)
        // + 1 (spread) + 1 (interp) + tail-pad. Recompute if MAX_STOPS or
        // Stop layout changes. Pinned to catch unintended layout drift.
        assert_eq!(std::mem::size_of::<LinearGradient>(), 48);
    }

    #[test]
    fn linear_two_stop_authoring() {
        let g = LinearGradient::two_stop(0.0, Color::hex(0x1a1a2e), Color::hex(0x16213e));
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].offset(), 0.0);
        assert_eq!(g.stops[1].offset(), 1.0);
        assert_eq!(g.spread, Spread::Pad);
        assert_eq!(g.interp, Interp::Oklab);
        assert!(!g.is_noop());
    }

    #[test]
    fn linear_three_stop_authoring() {
        let g = LinearGradient::three_stop(
            std::f32::consts::PI / 2.0,
            Color::hex(0x000000),
            Color::hex(0x808080),
            Color::hex(0xffffff),
        );
        assert_eq!(g.stops.len(), 3);
        // 0.5 round-trips through u8 quantization (0.5 * 255 + 0.5 → 128 → 128/255 ≈ 0.5019).
        assert!((g.stops[1].offset() - 0.5).abs() < 1.0 / 255.0);
    }

    #[test]
    fn linear_all_transparent_is_noop() {
        let g =
            LinearGradient::two_stop(0.0, ColorU8::TRANSPARENT, ColorU8::rgba(255, 255, 255, 0));
        assert!(g.is_noop());
        assert!(Brush::Linear(g).is_noop());
    }

    #[test]
    #[should_panic(expected = "exceeds MAX_STOPS")]
    fn linear_too_many_stops_panics() {
        let many: Vec<Stop> = (0..=MAX_STOPS)
            .map(|i| Stop::new(i as f32 / 8.0, Color::WHITE))
            .collect();
        let _ = LinearGradient::new(0.0, many);
    }

    #[test]
    #[should_panic(expected = "at least 2 stops")]
    fn linear_one_stop_panics() {
        let _ = LinearGradient::new(0.0, [Stop::new(0.0, Color::WHITE)]);
    }

    #[test]
    fn linear_brush_animatable_snaps_on_t_one() {
        let g0 = LinearGradient::two_stop(0.0, Color::BLACK, Color::WHITE);
        let g1 = LinearGradient::two_stop(0.0, Color::WHITE, Color::BLACK);
        let a = Brush::Linear(g0);
        let b = Brush::Linear(g1);
        // t < 1.0 snaps to a; t >= 1.0 snaps to b.
        assert_eq!(Brush::lerp(a, b, 0.5), a);
        assert_eq!(Brush::lerp(a, b, 1.0), b);
    }

    #[test]
    fn radial_default_centered() {
        let g = RadialGradient::two_stop_centered(Color::WHITE, Color::BLACK);
        assert_eq!(g.center, Vec2::splat(0.5));
        assert_eq!(g.radius, Vec2::splat(0.5));
        assert_eq!(g.interp, Interp::Oklab);
        assert_eq!(g.spread, Spread::Pad);
        // axis packs (cx, cy, rx, ry).
        let a = g.axis();
        assert_eq!(a.lanes(), [0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn conic_default_linear_interp_per_variant() {
        let g =
            ConicGradient::two_stop_centered(Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
        // Per-variant default: conic prefers Linear interp (predictable
        // hue sweeps). Linear/Radial default to Oklab.
        assert_eq!(g.interp, Interp::Linear);
        let l = LinearGradient::two_stop(0.0, Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
        assert_eq!(l.interp, Interp::Oklab);
        let r =
            RadialGradient::two_stop_centered(Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
        assert_eq!(r.interp, Interp::Oklab);
    }

    #[test]
    fn conic_axis_packs_start_angle() {
        let g = ConicGradient::new(
            Vec2::new(0.4, 0.6),
            std::f32::consts::FRAC_PI_4,
            [
                Stop::new(0.0, Color::rgb(1.0, 0.0, 0.0)),
                Stop::new(1.0, Color::rgb(0.0, 0.0, 1.0)),
            ],
        );
        let [dx, dy, t0, _] = g.axis().lanes();
        // f16 quantization: 0.4/0.6 don't round-trip exactly; the
        // assertion is at f16 tolerance (~1/2048).
        assert!((dx - 0.4).abs() < 1e-3);
        assert!((dy - 0.6).abs() < 1e-3);
        assert!((t0 - std::f32::consts::FRAC_PI_4).abs() < 1e-3);
    }

    #[test]
    fn brush_radial_conic_noop_when_all_transparent() {
        let r = RadialGradient::two_stop_centered(ColorU8::TRANSPARENT, ColorU8::TRANSPARENT);
        let c = ConicGradient::two_stop_centered(ColorU8::TRANSPARENT, ColorU8::TRANSPARENT);
        assert!(Brush::Radial(r).is_noop());
        assert!(Brush::Conic(c).is_noop());
    }

    #[test]
    fn brush_radial_conic_as_solid_is_none() {
        let r =
            RadialGradient::two_stop_centered(Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
        let c =
            ConicGradient::two_stop_centered(Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
        assert!(Brush::Radial(r).as_solid().is_none());
        assert!(Brush::Conic(c).as_solid().is_none());
    }

    /// Brush hashing: variant tag distinguishes Radial vs Conic vs
    /// Linear even when stops match.
    #[test]
    fn brush_variant_tag_distinguishes_hash() {
        let stops = [
            Stop::new(0.0, Color::rgb(1.0, 0.0, 0.0)),
            Stop::new(1.0, Color::rgb(0.0, 0.0, 1.0)),
        ];
        let l = Brush::Linear(LinearGradient::new(0.0, stops));
        let r = Brush::Radial(RadialGradient::new(
            Vec2::splat(0.5),
            Vec2::splat(0.5),
            stops,
        ));
        let c = Brush::Conic(ConicGradient::new(Vec2::splat(0.5), 0.0, stops));
        assert_ne!(h(l), h(r));
        assert_ne!(h(r), h(c));
        assert_ne!(h(l), h(c));
    }

    #[test]
    #[should_panic(expected = "exceeds MAX_STOPS")]
    fn radial_too_many_stops_panics() {
        let many: Vec<Stop> = (0..=MAX_STOPS)
            .map(|i| Stop::new(i as f32 / 8.0, Color::WHITE))
            .collect();
        let _ = RadialGradient::new(Vec2::splat(0.5), Vec2::splat(0.5), many);
    }

    #[test]
    fn linear_brush_hash_stable_across_construction() {
        let g0 = LinearGradient::two_stop(0.5, Color::hex(0x336699), Color::hex(0xddaa44));
        let g1 = LinearGradient::two_stop(0.5, Color::hex(0x336699), Color::hex(0xddaa44));
        assert_eq!(h(Brush::Linear(g0)), h(Brush::Linear(g1)));
    }
}
