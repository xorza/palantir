//! The radial gradient's axis: color runs outward from a center, with a radius per axis for ellipses.

use crate::primitives::math::float_hash::FloatHash;
use glam::Vec2;
use std::hash;

/// Geometry of a radial gradient: color runs outward from `center` along the elliptical `radius`, both in object-space 0..1 (origin top-left). The shader projects to `t = length((local01 - center) / radius)`, applies `Spread`, samples the LUT.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadialGeometry {
    /// Ramp origin, in object-space `0..1` coordinates.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::offset2")]
    pub center: Vec2,
    /// Elliptical radius, per axis, in the same coordinates. A `Vec2`, not a
    /// [`Size`](crate::Size): a pair of semi-axes the projection divides by, in object space, as
    /// kurbo's `Ellipse::radii` is, where a `Size` is an extent in logical px with an unbounded
    /// sentinel.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length2")]
    pub radius: Vec2,
}

impl RadialGeometry {
    /// The shader reads these as `(cx, cy, rx, ry)` on the radial branch.
    pub(crate) const fn axis_lanes(&self) -> [f32; 4] {
        [self.center.x, self.center.y, self.radius.x, self.radius.y]
    }

    pub(super) fn hash_visual<H: hash::Hasher>(&self, state: &mut H) {
        self.center.hash_visual(state);
        self.radius.hash_visual(state);
    }
}
