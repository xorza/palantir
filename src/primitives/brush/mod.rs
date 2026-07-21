use crate::animation::animatable::Animatable;
use crate::primitives::approx::canon_bits;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::half_simd::F16x4;
use glam::Vec2;
use tinyvec::ArrayVec;

mod serde;

/// Hard cap on stops in a single gradient. 8 covers >99% of UI use
/// (2-3 stops dominate, multi-stop bars rarely exceed 5).
pub(crate) const MAX_STOPS: usize = 8;

/// GPU-wire form of a gradient's axis: four f16 lanes (`[u16; 4]`,
/// 8 B). Variant-dependent layout — `[dir_x, dir_y, t0, t1]` for
/// linear, `[cx, cy, rx, ry]` for radial, `[cx, cy, start_angle, _]`
/// for conic, `[0, 0, σ, spread]` for drop shadows, and
/// `[offset.x, offset.y, σ, spread]` for inset shadows. Mirrors
/// `Corners`'s u64 lane scheme — the WGSL vertex attribute is
/// `vec2<u32>` and the shader unpacks via two `unpack2x16float`
/// calls into the same `vec4<f32>` the fragment shader sees.
///
/// f16 precision (~3 decimal digits) is plenty for unit direction
/// vectors and the 0..1 parametric range; sub-pixel error envelope
/// up to ~2048 px, then degrading like `Corners`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FillAxis(F16x4);

impl FillAxis {
    /// All-zero axis used for solid quads. The shader ignores it when
    /// `FillKind == SOLID`, so the value doesn't matter — keep it
    /// zeroed so Pod-byte cache keys are deterministic for solid
    /// quads.
    pub(crate) const ZERO: Self = Self(F16x4::ZERO);

    /// Build from four runtime f32 lanes via the batched f16 slice
    /// path. Single SIMD instruction on F16C/fp16 targets.
    #[inline]
    pub(crate) fn from_lanes(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self(F16x4::from_lanes([a, b, c, d]))
    }

    /// All four lanes unpacked at once via the batched slice path —
    /// matches `Corners::as_array`.
    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        self.0.lanes()
    }

    /// Per-lane f32 setter helper for the composer's
    /// `current_transform.scale` multiply path. Re-quantizes via the
    /// scalar f16 round-trip.
    #[inline]
    pub(crate) fn scaled(self, s: f32) -> Self {
        let [a, b, c, d] = self.lanes();
        Self::from_lanes(a * s, b * s, c * s, d * s)
    }
}

/// One colour stop in a gradient. `offset_u8` is the 0..1 parametric
/// position quantized to 8 bits (256 levels — finer than the LUT it
/// bakes into). `color` is 8-bit linear RGB. Total 5 B / stop, align 1, so
/// `GradientStops` is 40 B inline vs. 64 B with f32 offsets.
/// Stops are storage-only (never animated; snap on morph), feed a u8
/// LUT, and out-of-range positions clamp at construction — 8-bit
/// precision is sufficient and saves ~24 B per gradient.
///
/// Serde uses the **float** `offset` (0..1) as the wire form, not the
/// internal `offset_u8` byte — theme authors write `offset = 0.5`,
/// matching how every other spatial value in the crate is authored;
/// the u8 quantization stays an implementation detail.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Stop {
    pub offset_u8: u8,
    pub color: ColorU8,
}

impl Stop {
    /// Construct a stop. Finite offsets are clamped to 0..=1 and
    /// quantized to u8 (round-to-nearest).
    #[inline]
    pub fn new(offset: f32, color: impl Into<ColorU8>) -> Self {
        assert!(offset.is_finite(), "gradient stop offset must be finite");
        let q = (offset.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Self {
            offset_u8: q,
            color: color.into(),
        }
    }

    /// Decode the stored quantized position back to a 0..1 f32 for
    /// consumers (atlas bake, axis calc) that interpolate in float.
    #[inline]
    pub const fn offset(self) -> f32 {
        self.offset_u8 as f32 / 255.0
    }
}

/// Inline gradient-stop sequence whose length is always two through eight.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradientStops(ArrayVec<[Stop; MAX_STOPS]>);

impl GradientStops {
    /// Collect stops into inline storage, panicking on an invalid count.
    pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self {
        let mut values = ArrayVec::new();
        for stop in stops {
            assert!(
                values.len() < MAX_STOPS,
                "gradient stop count exceeds MAX_STOPS = {MAX_STOPS}",
            );
            values.push(stop);
        }
        assert!(
            values.len() >= 2,
            "gradient requires at least 2 stops, got {}",
            values.len(),
        );
        Self(values)
    }
}

impl std::ops::Deref for GradientStops {
    type Target = [Stop];

    fn deref(&self) -> &Self::Target {
        self.0.as_slice()
    }
}

impl std::ops::DerefMut for GradientStops {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut_slice()
    }
}

impl std::hash::Hash for GradientStops {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(self.len() as u8);
        for stop in self.iter() {
            state.write_u64(((stop.color.to_u32() as u64) << 32) | u64::from(stop.offset_u8));
        }
    }
}

/// How the gradient repeats outside the 0..1 parametric range.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
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
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
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
/// Stops live inline via [`GradientStops`] so a
/// `LinearGradient` value is heap-free. Total size is 48 B.
///
/// **Not `Copy`** — the 40 B [`GradientStops`] made implicit per-frame
/// copies expensive through the recording chain; see `Brush`'s
/// comment for the auto-`Copy` audit story. `.clone()` is cheap
/// (one inline memcpy) — just explicit.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct LinearGradient {
    pub angle: f32,
    pub stops: GradientStops,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for LinearGradient {
    /// Hand-written: f32 fields (`angle`, per-stop `offset`) need
    /// canonical bit encoding via `canon_bits` so `-0.0` / `+0.0` and
    /// NaN bit patterns don't fragment cache keys. Used by command-
    /// buffer dedup; the atlas hashes `(stops, interp)` separately
    /// (variant-agnostic) in `gradient_atlas::GradientLutKey`.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Angle's canonical bits in the high lane; the shared
        // (spread/interp) tag in the low 16 bits.
        state.write_u64(
            ((canon_bits(self.angle) as u64) << 32) | gradient_tag(self.spread, self.interp),
        );
        std::hash::Hash::hash(&self.stops, state);
    }
}

impl LinearGradient {
    /// General constructor. Asserts 2..=MAX_STOPS stops.
    pub fn new(angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            angle,
            stops: GradientStops::new(stops),
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

    /// Gradient axis for the shader. `dir = (cos(angle), sin(angle))`;
    /// the shader projects each fragment's 0..1 object-local position
    /// onto `dir`, then maps the dot product through `(t0, t1)` to
    /// the LUT row.
    ///
    /// Slice 2 always emits `(t0, t1) = (0, 1)` and the raw
    /// `(cos, sin)` axis; diagonal gradients project to a sub-1.0
    /// range and rely on `Spread::Pad` to clamp. CSS-style
    /// corner-to-corner scaling is a slice 2.5 polish task.
    pub(crate) fn axis(&self) -> FillAxis {
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
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct RadialGradient {
    pub center: Vec2,
    pub radius: Vec2,
    pub stops: GradientStops,
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
        state.write_u64(gradient_tag(self.spread, self.interp));
        std::hash::Hash::hash(&self.stops, state);
    }
}

impl RadialGradient {
    pub fn new(center: Vec2, radius: Vec2, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            center,
            radius,
            stops: GradientStops::new(stops),
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

    /// Pack `(center, radius)` into a `FillAxis` wire slot. The shader
    /// reads it as `(cx, cy, rx, ry)` for the radial branch.
    pub(crate) fn axis(&self) -> FillAxis {
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
/// `Oklch{hue}` interp would be the truly right default.)
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct ConicGradient {
    pub center: Vec2,
    pub start_angle: f32,
    pub stops: GradientStops,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for ConicGradient {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            ((canon_bits(self.center.x) as u64) << 32) | (canon_bits(self.center.y) as u64),
        );
        // `start_angle` bits in the high lane; shared tag in the low —
        // same layout as `LinearGradient`.
        state.write_u64(
            ((canon_bits(self.start_angle) as u64) << 32) | gradient_tag(self.spread, self.interp),
        );
        std::hash::Hash::hash(&self.stops, state);
    }
}

impl ConicGradient {
    pub fn new(center: Vec2, start_angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            center,
            start_angle,
            stops: GradientStops::new(stops),
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

    /// Pack `(center, start_angle)` into a `FillAxis` wire slot. The
    /// shader reads it as `(cx, cy, start_angle, _)` for the conic
    /// branch; `t1` is unused.
    pub(crate) fn axis(&self) -> FillAxis {
        FillAxis::from_lanes(self.center.x, self.center.y, self.start_angle, 0.0)
    }
}

/// `(spread, interp)` packed into the low 16 bits of a `u64` — the
/// shared tag every gradient hash writes. Linear/Conic OR it with
/// `canon_bits(angle) << 32`; Radial writes it standalone after its
/// centre/radius words. `GradientStops::hash` carries the stop count.
#[inline]
const fn gradient_tag(spread: Spread, interp: Interp) -> u64 {
    ((spread as u64) << 8) | interp as u64
}

/// Generate the builder + `is_noop` methods shared verbatim by all
/// three gradient variants. The fields (`stops`/`spread`/`interp`) stay
/// direct on each struct — external consumers read them positionally —
/// so only the identical method bodies are centralized here.
macro_rules! gradient_common {
    ($($t:ty),+ $(,)?) => {$(
        impl $t {
            /// Override how the gradient repeats outside the 0..1
            /// parametric range. Builder-style.
            pub const fn with_spread(mut self, spread: Spread) -> Self {
                self.spread = spread;
                self
            }

            /// Override the colour space interpolation runs in.
            /// Builder-style.
            pub const fn with_interp(mut self, interp: Interp) -> Self {
                self.interp = interp;
                self
            }

            /// Paints nothing visible when every stop is transparent.
            #[inline]
            pub fn is_noop(&self) -> bool {
                self.stops.iter().all(|s| s.color.is_noop())
            }
        }
    )+};
}

gradient_common!(LinearGradient, RadialGradient, ConicGradient);

/// Paint source for gradient-capable fills.
///
/// `Solid(Color)` is the hot 99% path — 16 B inline, animation-lerpable.
/// `Linear`/`Radial`/`Conic` carry their geometry inline (~80 B);
/// gradient morph animations snap across variants and across distinct
/// gradients of the same variant.
// `Brush` is intentionally **not `Copy`** — the gradient variants
// carry 40 B of inline stops and the whole enum is 60 B. The
// recording chain used to thread `Brush` (often inside `Background`)
// through 3-4 functions per chromed widget by value; auto-`Copy` hid
// an O(N) of `vmovups` per frame in `Ui::node`. Hot paths
// now pass `&Brush` / `&Background`; explicit `.clone()` at the
// remaining duplication sites keeps the cost auditable. See
// `Animatable`'s `Clone` (not `Copy`) supertrait for the matching
// animation-side relaxation.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub enum Brush {
    Solid(Color),
    Linear(LinearGradient),
    Radial(RadialGradient),
    Conic(ConicGradient),
}

/// Paint source for one-dimensional stroked shapes. Solid colors and linear
/// gradients have an unambiguous mapping along the curve parameter; radial and
/// conic gradients do not.
#[derive(Clone, Debug, PartialEq)]
pub enum CurveBrush {
    Solid(Color),
    Linear(LinearGradient),
}

impl CurveBrush {
    pub(crate) const TRANSPARENT: Self = Self::Solid(Color::TRANSPARENT);

    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        match self {
            CurveBrush::Solid(color) => color.is_noop(),
            CurveBrush::Linear(gradient) => gradient.is_noop(),
        }
    }
}

impl From<Color> for CurveBrush {
    #[inline]
    fn from(color: Color) -> Self {
        CurveBrush::Solid(color)
    }
}

impl From<LinearGradient> for CurveBrush {
    #[inline]
    fn from(gradient: LinearGradient) -> Self {
        CurveBrush::Linear(gradient)
    }
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
    /// `None` for gradient variants. Takes `&self` so callers with a borrowed
    /// `Brush` don't need to clone just to pull out the solid color.
    #[inline]
    pub const fn as_solid(&self) -> Option<Color> {
        match self {
            Brush::Solid(c) => Some(*c),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => None,
        }
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
        // Match on `(&a, &b)` instead of `(a, b)` so the gradient
        // fallback can still hand back one of the originals without
        // re-`Clone` — the tuple-by-value pattern used to work via
        // `Brush: Copy`, but the trait now requires only `Clone`.
        match (&a, &b) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(Color::lerp(*x, *y, t)),
            // Gradient morphs snap until interpolation between gradient payloads exists.
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
        match (&self, &other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.sub(*y)),
            _ => Self::zero(),
        }
    }
    #[inline]
    fn add(self, other: Self) -> Self {
        match (&self, &other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.add(*y)),
            _ => self,
        }
    }
    #[inline]
    fn scale(self, k: f32) -> Self {
        match self {
            Brush::Solid(c) => Brush::Solid(c.scale(k)),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => Self::zero(),
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
    #[inline]
    fn normalize_for_spring(&mut self, target: &Self, velocity: &mut Self) {
        if !matches!((&*self, target), (Brush::Solid(_), Brush::Solid(_))) {
            if self != target {
                *self = target.clone();
            }
            *velocity = Self::zero();
        }
    }
}

#[cfg(test)]
mod tests;
