//! The radial gradient's axis: colour runs outward from a centre, with a
//! radius per axis so an ellipse is expressible.

use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::gradient_builder::GradientBuilder;
use crate::primitives::paint::brush::gradient::sealed::Geometry;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::brush::gradient::{Gradient, Interpolation};
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;
use std::hash;

/// Geometry of a radial gradient: colour runs outward from `center`
/// along the elliptical radius `radius`. Both are object-space 0..1
/// coordinates (origin top-left, (1,1) bottom-right of the brush owner).
/// The shader projects each fragment to
/// `t = length((local01 - center) / radius)`, applies `Spread`, and
/// samples the LUT.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct RadialGeometry {
    /// Ramp origin, in object-space `0..1` coordinates.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset2")]
    pub center: Vec2,
    /// Elliptical radius, per axis, in the same coordinates.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length2")]
    pub radius: Vec2,
}

/// Radial gradient — see [`RadialGeometry`] for the projection.
pub type RadialGradient = Gradient<RadialGeometry>;

/// Authoring builder for a [`RadialGradient`].
pub type RadialGradientBuilder = GradientBuilder<RadialGeometry>;

impl Geometry for RadialGeometry {
    /// Radial fills are usually soft glows, where perceptual smoothness
    /// matters most.
    const DEFAULT_INTERPOLATION: Interpolation = Interpolation::Oklab;

    /// The shader reads these as `(cx, cy, rx, ry)` on the radial
    /// branch.
    fn axis_lanes(&self) -> [f32; 4] {
        [self.center.x, self.center.y, self.radius.x, self.radius.y]
    }

    fn hash_geometry<H: hash::Hasher>(&self, state: &mut H) {
        self.center.hash_visual(state);
        self.radius.hash_visual(state);
    }

    fn has_nan(&self) -> bool {
        self.center.has_nan() || self.radius.has_nan()
    }
}

impl RadialGradient {
    /// Start an inline, allocation-free gradient builder.
    pub fn builder(center: Vec2, radius: Vec2) -> RadialGradientBuilder {
        GradientBuilder::new(RadialGeometry { center, radius })
    }

    /// General constructor. Asserts two through eight stops.
    pub fn new(center: Vec2, radius: Vec2, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self::from_stops(RadialGeometry { center, radius }, stops)
    }

    /// 2-stop centred shorthand — `center = (0.5, 0.5)`,
    /// `radius = (0.5, 0.5)` (covers the bounding circle inscribed in
    /// the unit square). `c0` at offset 0 (centre), `c1` at offset 1
    /// (edge).
    pub fn two_stop(c0: RgbaF32, c1: RgbaF32) -> Self {
        Self::new(
            Vec2::splat(0.5),
            Vec2::splat(0.5),
            [Stop::new(0.0, c0), Stop::new(1.0, c1)],
        )
    }
}
