//! The conic gradient's axis: colour sweeps around a centre by angle.

use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::gradient_builder::GradientBuilder;
use crate::primitives::paint::brush::gradient::sealed::Geometry;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::brush::gradient::{Gradient, Interpolation};
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;
use std::hash;

/// Geometry of a conic (sweep) gradient: the 0..1 axis sweeps around `center` from `start_angle` radians, clockwise on screen (angles grow from +x toward +y, which points down). `center` is in 0..1 object space. The shader projects each fragment to `t = fract((atan2(dy, dx) - start_angle) / TAU + 1.0)`, applies `Spread`, samples the LUT.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct ConicGeometry {
    /// Sweep centre, in object-space `0..1` coordinates.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset2")]
    pub center: Vec2,
    /// Where the sweep begins, in radians.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::angle")]
    pub start_angle: f32,
}

/// Conic (sweep) gradient — see [`ConicGeometry`] for the sweep.
pub type ConicGradient = Gradient<ConicGeometry>;

/// Authoring builder for a [`ConicGradient`].
pub type ConicGradientBuilder = GradientBuilder<ConicGeometry>;

impl Geometry for ConicGeometry {
    /// Conic gradients are usually colour wheels, where linear-RGB interpolation gives the most predictable hue sweep; Oklab can shift hue at the midpoint. (An `Oklch{hue}` interpolation would be the right default.)
    const DEFAULT_INTERPOLATION: Interpolation = Interpolation::Linear;

    /// The shader reads these as `(cx, cy, start_angle, _)` on the conic branch.
    fn axis_lanes(&self) -> [f32; 4] {
        [self.center.x, self.center.y, self.start_angle, 0.0]
    }

    fn hash_geometry<H: hash::Hasher>(&self, state: &mut H) {
        self.center.hash_visual(state);
        self.start_angle.hash_visual(state);
    }

    fn has_nan(&self) -> bool {
        self.center.has_nan() || self.start_angle.is_nan()
    }
}

impl ConicGradient {
    /// Start an inline, allocation-free gradient builder.
    pub fn builder(center: Vec2, start_angle: f32) -> ConicGradientBuilder {
        GradientBuilder::new(ConicGeometry {
            center,
            start_angle,
        })
    }

    /// General constructor. Asserts two through eight stops.
    pub fn new(center: Vec2, start_angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self::from_stops(
            ConicGeometry {
                center,
                start_angle,
            },
            stops,
        )
    }

    /// Centred shorthand: `center = (0.5, 0.5)`, start angle 0 (+x, clockwise), 2 stops at offsets 0/1.
    pub fn two_stop(c0: RgbaF32, c1: RgbaF32) -> Self {
        Self::new(
            Vec2::splat(0.5),
            0.0,
            [Stop::new(0.0, c0), Stop::new(1.0, c1)],
        )
    }
}
