//! The linear gradient's axis: colour runs along the direction an angle names.

use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::paint::brush::gradient::gradient_builder::GradientBuilder;
use crate::primitives::paint::brush::gradient::sealed::Geometry;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::brush::gradient::{Gradient, Interpolation};
use crate::primitives::paint::color::RgbaF32;
use std::hash;

/// Geometry of a linear gradient: colour runs along `angle` radians (0 = →, π/2 = ↓), spanning the brush owner's bounding rect end to end.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct LinearGeometry {
    /// Axis direction in radians.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::angle")]
    pub angle: f32,
}

/// Linear gradient — see [`LinearGeometry`] for the axis convention.
pub type LinearGradient = Gradient<LinearGeometry>;

/// Authoring builder for a [`LinearGradient`].
pub type LinearGradientBuilder = GradientBuilder<LinearGeometry>;

impl Geometry for LinearGeometry {
    const DEFAULT_INTERPOLATION: Interpolation = Interpolation::Oklab;

    /// `dir = (cos(angle), sin(angle))`; the shader projects each 0..1 object-local position onto `dir` and maps the dot through `(t0, t1)` to the LUT row. `(t0, t1)` is always `(0, 1)` over the raw axis, so a diagonal spans a sub-1.0 range and relies on `Spread::Pad`, unlike CSS corner-to-corner scaling.
    fn axis_lanes(&self) -> [f32; 4] {
        let (sin, cos) = self.angle.sin_cos();
        [cos, sin, 0.0, 1.0]
    }

    fn hash_geometry<H: hash::Hasher>(&self, state: &mut H) {
        self.angle.hash_visual(state);
    }

    fn has_nan(&self) -> bool {
        self.angle.is_nan()
    }
}

impl LinearGradient {
    /// Start an inline, allocation-free gradient builder.
    pub fn builder(angle: f32) -> LinearGradientBuilder {
        GradientBuilder::new(LinearGeometry { angle })
    }

    /// General constructor. Asserts two through eight stops.
    pub fn new(angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self::from_stops(LinearGeometry { angle }, stops)
    }

    /// 2-stop shorthand: `c0` at offset 0, `c1` at 1; the common UI-gradient case.
    pub fn two_stop(angle: f32, c0: RgbaF32, c1: RgbaF32) -> Self {
        Self::new(angle, [Stop::new(0.0, c0), Stop::new(1.0, c1)])
    }
}
