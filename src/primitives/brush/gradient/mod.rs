//! Gradients: one [`Gradient`] type parameterized by the geometry payload
//! that distinguishes the linear, radial and conic kinds.
//!
//! Everything a gradient carries beside that payload — the stop list, the
//! spread mode, the interpolation space, the builder, the cache-key hash
//! and the NaN screen — is identical across the three kinds and is
//! written once here.

use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::half_simd::F16x4;
use crate::primitives::nan::NanCheck;

pub(crate) mod conic_geometry;
pub(crate) mod gradient_builder;
pub(crate) mod linear_geometry;
pub(crate) mod radial_geometry;
pub(crate) mod stops;

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

/// The per-kind half of a gradient: the geometry the shader projects a
/// fragment onto, plus the two policies that follow from it.
///
/// One implementor per gradient kind, each in its own file beside the
/// [`Gradient`] alias it names.
pub trait GradientGeometry {
    /// Interpolation space a freshly authored gradient of this kind
    /// starts in, before [`Gradient::with_interp`] overrides it.
    const DEFAULT_INTERP: Interp;

    /// The four axis lanes the shader reads, before `FillAxis` packs
    /// them to f16. The layout is per-kind.
    fn axis_lanes(&self) -> [f32; 4];

    /// Fold the geometry into a cache key.
    ///
    /// f32 fields go through `approx::canon_bits`, so `-0.0` / `+0.0` and
    /// NaN bit patterns don't fragment command-buffer dedup.
    fn hash_geometry<H: std::hash::Hasher>(&self, state: &mut H);

    /// Whether the geometry holds a NaN.
    fn has_nan(&self) -> bool;
}

/// A gradient of any kind: `geometry` picks the kind, and the rest is
/// the same for all three.
///
/// Stops live inline via [`GradientStops`] so a gradient value is
/// heap-free — 48 B for the linear kind, 60 B for the radial one.
///
/// **Not `Copy`** — the 40 B [`GradientStops`] made implicit per-frame
/// copies expensive through the recording chain; see `Brush`'s comment
/// for the auto-`Copy` audit story. `.clone()` is cheap (one inline
/// memcpy) — just explicit.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct Gradient<G> {
    #[serde(flatten)]
    pub geometry: G,
    pub stops: GradientStops,
    pub spread: Spread,
    pub interp: Interp,
}

impl<G> Gradient<G> {
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
        self.stops.iter().all(|stop| stop.color.is_noop())
    }
}

impl<G: GradientGeometry> Gradient<G> {
    /// The one general constructor the per-kind `new` shorthands land in.
    /// Asserts two through eight stops.
    fn from_stops(geometry: G, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            geometry,
            stops: GradientStops::new(stops),
            spread: Spread::default(),
            interp: G::DEFAULT_INTERP,
        }
    }

    /// Gradient axis for the shader, packed to the GPU wire form.
    pub(crate) fn axis(&self) -> FillAxis {
        let [a, b, c, d] = self.geometry.axis_lanes();
        FillAxis::from_lanes(a, b, c, d)
    }
}

/// Hand-written rather than derived: the geometry needs canonical f32
/// bit encoding, and the stops hash through their own packed form. Used
/// by command-buffer dedup; the atlas hashes `(stops, interp)` separately
/// (kind-agnostic) in `gradient_atlas::GradientLutKey`.
impl<G: GradientGeometry> std::hash::Hash for Gradient<G> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.geometry.hash_geometry(state);
        state.write_u64(gradient_tag(self.spread, self.interp));
        std::hash::Hash::hash(&self.stops, state);
    }
}

/// Stop offsets and colours are integer-encoded (`Stop::offset_u8`,
/// `ColorU8`), so a gradient's geometry is the only place a NaN can hide.
impl<G: GradientGeometry> NanCheck for Gradient<G> {
    #[inline]
    fn has_nan(&self) -> bool {
        self.geometry.has_nan()
    }
}

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

impl From<F16x4> for FillAxis {
    /// Adopt an already-packed word. The inset-shadow axis is exactly
    /// `LoweredShadow::geom_f16`, so it travels packed rather than
    /// through an f16 → f32 → f16 round trip of identical bytes.
    #[inline]
    fn from(lanes: F16x4) -> Self {
        Self(lanes)
    }
}

impl FillAxis {
    /// All-zero axis used for solid quads. The shader ignores it when
    /// `FillKind == SOLID`, so the value doesn't matter — keep it
    /// zeroed so Pod-byte cache keys are deterministic for solid
    /// quads.
    pub(crate) const ZERO: Self = Self(F16x4::ZERO);

    /// Build from four runtime f32 lanes. Single SIMD instruction on
    /// F16C/fp16 targets.
    #[inline]
    pub(crate) fn from_lanes(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self(F16x4::from_lanes([a, b, c, d]))
    }

    /// All four lanes unpacked at once — matches `Corners::as_array`.
    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        self.0.lanes()
    }

    /// Scale every lane — the composer's walk-transform scale
    /// multiply, run per quad.
    ///
    /// Delegates to [`F16x4::scaled`] rather than composing
    /// `from_lanes(lanes().map(..))`: that spelling bounces through two
    /// `[f32; 4]` arrays and measures 1.3x slower, which is the whole
    /// reason the fused form exists. `Corners::scaled_by` delegates the
    /// same way.
    #[inline]
    pub(crate) fn scaled(self, s: f32) -> Self {
        Self(self.0.scaled(s))
    }
}

#[inline]
const fn gradient_tag(spread: Spread, interp: Interp) -> u64 {
    ((spread as u64) << 8) | interp as u64
}
