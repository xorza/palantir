//! The conic gradient's axis: color sweeps around a center by angle.

use crate::primitives::math::float_hash::FloatHash;
use glam::Vec2;
use std::f32::consts::TAU;
use std::hash;

/// Geometry of a conic (sweep) gradient: the 0..1 axis sweeps around `center` from `start_angle` radians, clockwise on screen (angles grow from +x toward +y, which points down). `center` is in 0..1 object space. The shader projects each fragment to `t = fract((atan2(dy, dx) - start_angle) / TAU + 1.0)`, applies `Spread`, samples the LUT.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConicGeometry {
    /// Sweep center, in object-space `0..1` coordinates.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::offset2")]
    pub center: Vec2,
    /// Where the sweep begins, in radians.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::angle")]
    pub start_angle: f32,
}

impl ConicGeometry {
    /// The shader reads these as `(cx, cy, start_angle, _)` on the conic branch. The angle is
    /// wrapped into one turn first: f16 steps 0.5 rad apart at 1000, so a raw large angle would turn
    /// the sweep.
    pub(crate) fn axis_lanes(&self) -> [f32; 4] {
        [
            self.center.x,
            self.center.y,
            self.start_angle.rem_euclid(TAU),
            0.0,
        ]
    }

    pub(super) fn hash_visual<H: hash::Hasher>(&self, state: &mut H) {
        self.center.hash_visual(state);
        self.start_angle.hash_visual(state);
    }
}
