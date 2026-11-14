//! Gradients: one [`Gradient`] over the [`GradientGeometry`] that tells linear, radial and conic
//! apart; everything else is shared.

use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::conic_geometry::ConicGeometry;
use crate::primitives::paint::brush::gradient::gradient_geometry::GradientGeometry;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGeometry;
use crate::primitives::paint::brush::gradient::radial_geometry::RadialGeometry;
use glam::Vec2;
use std::f32::consts::FRAC_PI_2;
use std::hash;

pub(crate) mod color_ramp;
pub(crate) mod conic_geometry;
pub(crate) mod gradient_geometry;
pub(crate) mod linear_geometry;
pub(crate) mod radial_geometry;
pub(crate) mod stops;

#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
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

/// Color space the interpolation runs in; stop colors are unchanged.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub enum Interpolation {
    /// Perceptually uniform; the CSS Color 4 default. Avoids the muddy midpoint
    /// of complementary pairs.
    #[default]
    Oklab,
    /// Linear-RGB. Cheapest, but dips at the midpoint of saturated complementary
    /// pairs.
    Linear,
}

/// A gradient of any kind: `geometry` maps each fill point to a value,
/// `spread` folds values outside `0..=1` back in, and `ramp` maps the value to
/// a color. One type over an enum of the three geometries, as peniko's `Gradient` holds its
/// `GradientKind` ([peniko](https://docs.rs/peniko/latest/peniko/struct.Gradient.html)).
/// Refused: a type per kind, generic over a sealed geometry trait, which exports a name per kind
/// and gives `Brush` a variant per kind.
///
/// **Not `Copy`**: the inline [`GradientStops`](crate::GradientStops) made implicit copies expensive
/// through the recording chain (see `Brush`). `.clone()` is one memcpy.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[must_use]
pub struct Gradient {
    /// Where the gradient runs.
    pub geometry: GradientGeometry,
    /// The colors along that axis.
    pub ramp: ColorRamp,
    /// What happens outside `0..=1`.
    pub spread: Spread,
}

impl Gradient {
    /// A gradient over `geometry` through `ramp`: `ramp` an array of colors spread evenly, an
    /// array of [`Stop`](crate::Stop)s placed where they say, or a [`ColorRamp`]. Spread
    /// [`Spread::Pad`]; the kinds' constructors land here.
    ///
    /// # Panics
    ///
    /// As [`ColorRamp::new`](crate::ColorRamp::new).
    #[track_caller]
    pub fn new(geometry: GradientGeometry, ramp: impl Into<ColorRamp>) -> Self {
        Self {
            geometry,
            ramp: ramp.into(),
            spread: Spread::Pad,
        }
    }

    /// A linear gradient along `angle` radians ([`LinearGeometry`]).
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn linear(angle: f32, ramp: impl Into<ColorRamp>) -> Self {
        Self::new(GradientGeometry::Linear(LinearGeometry { angle }), ramp)
    }

    /// Left to right, angle 0, as Compose's `Brush.horizontalGradient`.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn horizontal(ramp: impl Into<ColorRamp>) -> Self {
        Self::linear(0.0, ramp)
    }

    /// Top to bottom, angle π/2, as Compose's `Brush.verticalGradient`.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn vertical(ramp: impl Into<ColorRamp>) -> Self {
        Self::linear(FRAC_PI_2, ramp)
    }

    /// A radial gradient out from `center` to `radius` ([`RadialGeometry`]).
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn radial(center: Vec2, radius: Vec2, ramp: impl Into<ColorRamp>) -> Self {
        Self::new(
            GradientGeometry::Radial(RadialGeometry { center, radius }),
            ramp,
        )
    }

    /// The inscribed ellipse: from the center out to each side's midpoint.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn centered_radial(ramp: impl Into<ColorRamp>) -> Self {
        Self::radial(Vec2::splat(0.5), Vec2::splat(0.5), ramp)
    }

    /// A sweep around `center` from `start_angle` radians, one turn through the ramp
    /// ([`ConicGeometry`]).
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn conic(center: Vec2, start_angle: f32, ramp: impl Into<ColorRamp>) -> Self {
        Self::new(
            GradientGeometry::Conic(ConicGeometry {
                center,
                start_angle,
            }),
            ramp,
        )
    }

    /// About the center, from angle 0, pointing right.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn centered_conic(ramp: impl Into<ColorRamp>) -> Self {
        Self::conic(Vec2::splat(0.5), 0.0, ramp)
    }

    /// Override how the gradient fills outside 0..1.
    pub const fn with_spread(mut self, spread: Spread) -> Self {
        self.spread = spread;
        self
    }

    /// Override the color space interpolation runs in.
    pub const fn with_interpolation(mut self, interpolation: Interpolation) -> Self {
        self.ramp.interpolation = interpolation;
        self
    }

    /// Paints nothing visible when every stop is transparent.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.ramp.is_noop()
    }

    pub(crate) fn axis(&self) -> FillAxis {
        FillAxis::from_lanes(self.geometry.axis_lanes())
    }

    /// The fill kind the shader branches on, with the spread packed in.
    pub(crate) const fn fill_kind(&self) -> FillKind {
        match self.geometry {
            GradientGeometry::Linear(_) => FillKind::linear(self.spread),
            GradientGeometry::Radial(_) => FillKind::radial(self.spread),
            GradientGeometry::Conic(_) => FillKind::conic(self.spread),
        }
    }
}

/// Hand-written: the geometry needs canonical f32 bit encoding and the stops
/// hash through their packed form. For command-buffer dedup; the atlas keys on
/// the kind-agnostic [`ColorRamp`] alone.
impl hash::Hash for Gradient {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.geometry.hash_visual(state);
        state.write_u64(gradient_tag(self.spread, self.ramp.interpolation));
        hash::Hash::hash(&self.ramp.stops, state);
    }
}

/// Stop offsets and colors are integer-encoded, so the geometry is the only
/// place a NaN can hide.
impl NanCheck for Gradient {
    #[inline]
    fn has_nan(&self) -> bool {
        self.geometry.has_nan()
    }
}

#[inline]
const fn gradient_tag(spread: Spread, interpolation: Interpolation) -> u64 {
    ((spread as u64) << 8) | interpolation as u64
}
