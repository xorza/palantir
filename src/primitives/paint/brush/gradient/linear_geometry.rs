//! The linear gradient's axis: color runs along the direction an angle names.

use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::libm_ext::Libm;
use std::hash;

/// Geometry of a linear gradient: color runs along `angle` radians (0 = →, π/2 = ↓), spanning the brush owner's bounding rect end to end.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearGeometry {
    /// Axis direction in radians.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::angle")]
    pub angle: f32,
}

impl LinearGeometry {
    /// `dir = (cos(angle), sin(angle))`; the shader projects each 0..1 object-local position onto `dir` and maps the dot through `(t0, t1)` to the LUT row. `(t0, t1)` is the projection of the unit square onto `dir`, so at every angle stop 0 lands on the trailing corner and stop 1 on the leading one. The angle is in the box's 0..1 space, not in pixels as CSS measures it, so on a box that is not square a diagonal runs corner to corner.
    pub(crate) fn axis_lanes(self) -> [f32; 4] {
        let (sin, cos) = self.angle.libm_sin_cos();
        [
            cos,
            sin,
            cos.min(0.0) + sin.min(0.0),
            cos.max(0.0) + sin.max(0.0),
        ]
    }

    pub(super) fn hash_visual<H: hash::Hasher>(self, state: &mut H) {
        self.angle.hash_visual(state);
    }
}
