//! The conic gradient's axis: color sweeps around a center by angle.

use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::sealed::Geometry;
use glam::Vec2;
use std::f32::consts::TAU;
use std::hash;

/// Geometry of a conic (sweep) gradient: the 0..1 axis sweeps around `center` from `start_angle` radians, clockwise on screen (angles grow from +x toward +y, which points down). `center` is in 0..1 object space. The shader projects each fragment to `t = fract((atan2(dy, dx) - start_angle) / TAU + 1.0)`, applies `Spread`, samples the LUT.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct ConicGeometry {
    /// Sweep center, in object-space `0..1` coordinates.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::offset2")]
    pub center: Vec2,
    /// Where the sweep begins, in radians.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::angle")]
    pub start_angle: f32,
}

/// Conic (sweep) gradient — see [`ConicGeometry`] for the sweep.
pub type ConicGradient = Gradient<ConicGeometry>;

impl Geometry for ConicGeometry {
    /// The shader reads these as `(cx, cy, start_angle, _)` on the conic branch. The angle is
    /// wrapped into one turn first: f16 steps 0.5 rad apart at 1000, so a raw large angle would turn
    /// the sweep.
    fn axis_lanes(&self) -> [f32; 4] {
        [
            self.center.x,
            self.center.y,
            self.start_angle.rem_euclid(TAU),
            0.0,
        ]
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
    /// A sweep around `center` from `start_angle` radians, one turn through `ramp`: `ramp` an array of colors
    /// spread evenly, an array of [`Stop`](crate::Stop)s placed where they say, or a [`ColorRamp`].
    ///
    /// # Panics
    ///
    /// As [`ColorRamp::new`](crate::ColorRamp::new).
    #[track_caller]
    pub fn new(center: Vec2, start_angle: f32, ramp: impl Into<ColorRamp>) -> Self {
        Self::from_ramp(
            ConicGeometry {
                center,
                start_angle,
            },
            ramp.into(),
        )
    }

    /// About the center, from angle 0, pointing right.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn centered(ramp: impl Into<ColorRamp>) -> Self {
        Self::new(Vec2::splat(0.5), 0.0, ramp)
    }
}
