//! Gradients: one [`Gradient`] type parameterized by the geometry payload
//! that distinguishes the linear, radial and conic kinds.
//!
//! Everything a gradient carries beside that payload — the colour ramp,
//! the spread mode, the builder, the cache-key hash and the NaN screen —
//! is identical across the three kinds and is written once here.

use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, Stop};
use std::hash;

pub(crate) mod color_ramp;
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
    /// Perceptually uniform; matches CSS RgbaF32 4 default. Avoids the
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
/// fragment onto. One implementor per gradient kind — [`LinearGeometry`],
/// [`RadialGeometry`] and [`ConicGeometry`] — each beside the [`Gradient`]
/// alias it names.
///
/// Sealed: the renderer draws exactly these three kinds, so an outside
/// implementation could not paint. The items live on `sealed::Geometry`,
/// in a module private to the crate; the public trait is only the bound on
/// [`Gradient`].
///
/// [`LinearGeometry`]: crate::LinearGeometry
/// [`RadialGeometry`]: crate::RadialGeometry
/// [`ConicGeometry`]: crate::ConicGeometry
pub trait GradientGeometry: sealed::Geometry {}

impl<T: sealed::Geometry> GradientGeometry for T {}

pub(crate) mod sealed {
    use crate::primitives::paint::brush::gradient::Interp;
    use std::hash;

    /// The items behind [`GradientGeometry`](super::GradientGeometry): the
    /// renderer's view of one gradient kind.
    pub trait Geometry {
        /// Interpolation space a freshly authored gradient of this kind
        /// starts in, before `Gradient::with_interp` overrides it.
        const DEFAULT_INTERP: Interp;

        /// The four axis lanes the shader reads, before `FillAxis` packs
        /// them to f16. The layout is per-kind.
        fn axis_lanes(&self) -> [f32; 4];

        /// Fold the geometry into a cache key.
        ///
        /// f32 fields go through `float_hash::canon_bits`, so `-0.0` /
        /// `+0.0` and NaN bit patterns don't fragment command-buffer dedup.
        fn hash_geometry<H: hash::Hasher>(&self, state: &mut H);

        /// Whether the geometry holds a NaN.
        fn has_nan(&self) -> bool;
    }
}

/// A gradient of any kind: `geometry` maps each point of the fill to a
/// value, `spread` folds values outside `0..=1` back in, and `ramp` maps
/// the value to a colour. Only the geometry differs between the kinds.
///
/// Stops live inline via [`GradientStops`] so a gradient value is
/// heap-free — 48 B for the linear kind, 60 B for the radial one.
///
/// **Not `Copy`** — the 40 B [`GradientStops`] made implicit per-frame
/// copies expensive through the recording chain; see `Brush`'s comment
/// for the auto-`Copy` audit story. `.clone()` is cheap (one inline
/// memcpy) — just explicit.
// `repr(C)`, here and on `ColorRamp`, pins the two enum bytes last.
// `Brush` stores its tag in values `interp` or `spread` never take, which
// works only if that byte sits past the end of every smaller variant: in
// the 60 B radial kind they are bytes 57 and 58, and the linear and conic
// kinds are 48 B and 56 B. Free to reorder, rustc puts `interp` at byte
// 16, and `Brush` needs 64 B.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[must_use]
pub struct Gradient<G> {
    /// Where the parametric axis runs — see the geometry type.
    #[serde(flatten)]
    pub geometry: G,
    /// The colours along that axis.
    #[serde(flatten)]
    pub ramp: ColorRamp,
    /// What happens outside `0..=1`.
    pub spread: Spread,
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
        self.ramp.interp = interp;
        self
    }

    /// Paints nothing visible when every stop is transparent.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.ramp.is_noop()
    }
}

impl<G: GradientGeometry> Gradient<G> {
    /// The one general constructor the per-kind `new` shorthands land in.
    /// Asserts two through eight stops.
    fn from_stops(geometry: G, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            geometry,
            ramp: ColorRamp {
                stops: GradientStops::new(stops),
                interp: G::DEFAULT_INTERP,
            },
            spread: Spread::default(),
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
/// by command-buffer dedup; the atlas keys its rows on the [`ColorRamp`]
/// alone, which is kind-agnostic.
impl<G: GradientGeometry> hash::Hash for Gradient<G> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.geometry.hash_geometry(state);
        state.write_u64(gradient_tag(self.spread, self.ramp.interp));
        hash::Hash::hash(&self.ramp.stops, state);
    }
}

/// Stop offsets and colours are integer-encoded (`Stop::offset_u8`,
/// `SrgbaU8`), so a gradient's geometry is the only place a NaN can hide.
impl<G: GradientGeometry> NanCheck for Gradient<G> {
    #[inline]
    fn has_nan(&self) -> bool {
        self.geometry.has_nan()
    }
}

#[inline]
const fn gradient_tag(spread: Spread, interp: Interp) -> u64 {
    ((spread as u64) << 8) | interp as u64
}
