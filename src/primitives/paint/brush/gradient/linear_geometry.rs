//! The linear gradient's axis: color runs along the direction an angle names.

use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::libm_ext::Libm;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::sealed::Geometry;
use std::f32::consts::FRAC_PI_2;
use std::hash;

/// Geometry of a linear gradient: color runs along `angle` radians (0 = →, π/2 = ↓), spanning the brush owner's bounding rect end to end.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearGeometry {
    /// Axis direction in radians.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::angle")]
    pub angle: f32,
}

/// Linear gradient — see [`LinearGeometry`] for the axis convention.
pub type LinearGradient = Gradient<LinearGeometry>;

impl Geometry for LinearGeometry {
    /// `dir = (cos(angle), sin(angle))`; the shader projects each 0..1 object-local position onto `dir` and maps the dot through `(t0, t1)` to the LUT row. `(t0, t1)` is the projection of the unit square onto `dir`, so at every angle stop 0 lands on the trailing corner and stop 1 on the leading one. The angle is in the box's 0..1 space, not in pixels as CSS measures it, so on a box that is not square a diagonal runs corner to corner.
    fn axis_lanes(&self) -> [f32; 4] {
        let (sin, cos) = self.angle.libm_sin_cos();
        [
            cos,
            sin,
            cos.min(0.0) + sin.min(0.0),
            cos.max(0.0) + sin.max(0.0),
        ]
    }

    fn hash_geometry<H: hash::Hasher>(&self, state: &mut H) {
        self.angle.hash_visual(state);
    }

    fn has_nan(&self) -> bool {
        self.angle.is_nan()
    }
}

impl LinearGradient {
    /// A gradient along `angle` radians through `ramp`: `ramp` an array of colors
    /// spread evenly, an array of [`Stop`](crate::Stop)s placed where they say, or a [`ColorRamp`].
    ///
    /// # Panics
    ///
    /// As [`ColorRamp::new`](crate::ColorRamp::new).
    #[track_caller]
    pub fn new(angle: f32, ramp: impl Into<ColorRamp>) -> Self {
        Self::from_ramp(LinearGeometry { angle }, ramp.into())
    }

    /// Left to right, angle 0, as Compose's `Brush.horizontalGradient`.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn horizontal(ramp: impl Into<ColorRamp>) -> Self {
        Self::new(0.0, ramp)
    }

    /// Top to bottom, angle π/2, as Compose's `Brush.verticalGradient`.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn vertical(ramp: impl Into<ColorRamp>) -> Self {
        Self::new(FRAC_PI_2, ramp)
    }
}
