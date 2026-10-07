//! Gradients: one [`Gradient`] type parameterized by the geometry payload that
//! distinguishes linear, radial and conic; everything else is shared.

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

#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
)]
/// How a gradient fills past its ends.
pub enum Spread {
    /// Clamp to nearest edge stop. CSS default.
    #[default]
    Pad = 0,
    /// Tile 0..1 across the surface.
    Repeat = 1,
    /// Tile mirrored.
    Reflect = 2,
}

/// Colour space the interpolation runs in; stop colours are unchanged.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
)]
pub enum Interpolation {
    /// Perceptually uniform; the CSS Color 4 default. Avoids the muddy midpoint
    /// of complementary pairs.
    #[default]
    Oklab,
    /// Linear-RGB. Cheapest, but dips at the midpoint of saturated complementary
    /// pairs.
    Linear,
}

/// The per-kind half of a gradient: the geometry the shader projects a
/// fragment onto. One implementor per kind ([`LinearGeometry`],
/// [`RadialGeometry`], [`ConicGeometry`]).
///
/// Sealed: the renderer draws exactly these kinds. The items live on the
/// crate-private `sealed::Geometry`.
///
/// [`LinearGeometry`]: crate::LinearGeometry
/// [`RadialGeometry`]: crate::RadialGeometry
/// [`ConicGeometry`]: crate::ConicGeometry
pub trait GradientGeometry: sealed::Geometry {}

impl<T: sealed::Geometry> GradientGeometry for T {}

pub(crate) mod sealed {
    use crate::primitives::paint::brush::gradient::Interpolation;
    use std::hash;

    /// The renderer's view of one gradient kind.
    pub trait Geometry {
        /// Interpolation space a new gradient of this kind starts in.
        const DEFAULT_INTERPOLATION: Interpolation;

        /// The four axis lanes the shader reads, before `FillAxis` packs them to f16.
        fn axis_lanes(&self) -> [f32; 4];

        /// Fold the geometry into a cache key; f32 fields use `float_hash::canon_bits`
        /// so `-0.0`/`+0.0` and NaN patterns don't fragment dedup.
        fn hash_geometry<H: hash::Hasher>(&self, state: &mut H);

        /// Whether the geometry holds a NaN.
        fn has_nan(&self) -> bool;
    }
}

/// A gradient of any kind: `geometry` maps each fill point to a value,
/// `spread` folds values outside `0..=1` back in, and `ramp` maps the value to
/// a colour.
///
/// **Not `Copy`**: the inline [`GradientStops`] made implicit copies expensive
/// through the recording chain (see `Brush`). `.clone()` is one memcpy.
// `repr(C)`, here and on `ColorRamp`, pins the two enum bytes last: `Brush`
// stores its tag in values `interpolation` and `spread` never take, which only
// works if that byte sits past the end of every smaller variant.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[must_use]
pub struct Gradient<G> {
    #[serde(flatten)]
    /// Where the gradient runs.
    pub geometry: G,
    /// The colours along that axis.
    #[serde(flatten)]
    pub ramp: ColorRamp,
    /// What happens outside `0..=1`.
    pub spread: Spread,
}

impl<G> Gradient<G> {
    /// Override how the gradient repeats outside 0..1.
    pub const fn with_spread(mut self, spread: Spread) -> Self {
        self.spread = spread;
        self
    }

    /// Override the colour space interpolation runs in.
    pub const fn with_interpolation(mut self, interpolation: Interpolation) -> Self {
        self.ramp.interpolation = interpolation;
        self
    }

    /// Paints nothing visible when every stop is transparent.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.ramp.is_noop()
    }
}

impl<G: GradientGeometry> Gradient<G> {
    /// The one general constructor the per-kind `new` shorthands land in; asserts
    /// two through eight stops.
    fn from_stops(geometry: G, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            geometry,
            ramp: ColorRamp {
                stops: GradientStops::new(stops),
                interpolation: G::DEFAULT_INTERPOLATION,
            },
            spread: Spread::default(),
        }
    }

    pub(crate) fn axis(&self) -> FillAxis {
        let [a, b, c, d] = self.geometry.axis_lanes();
        FillAxis::from_lanes(a, b, c, d)
    }
}

/// Hand-written: the geometry needs canonical f32 bit encoding and the stops
/// hash through their packed form. For command-buffer dedup; the atlas keys on
/// the kind-agnostic [`ColorRamp`] alone.
impl<G: GradientGeometry> hash::Hash for Gradient<G> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.geometry.hash_geometry(state);
        state.write_u64(gradient_tag(self.spread, self.ramp.interpolation));
        hash::Hash::hash(&self.ramp.stops, state);
    }
}

/// Stop offsets and colours are integer-encoded, so the geometry is the only
/// place a NaN can hide.
impl<G: GradientGeometry> NanCheck for Gradient<G> {
    #[inline]
    fn has_nan(&self) -> bool {
        self.geometry.has_nan()
    }
}

#[inline]
const fn gradient_tag(spread: Spread, interpolation: Interpolation) -> u64 {
    ((spread as u64) << 8) | interpolation as u64
}
