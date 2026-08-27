use crate::primitives::approx::FloatHash;
use crate::primitives::brush::gradient::gradient_builder::GradientBuilder;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::brush::gradient::{Gradient, GradientGeometry, Interp};
use crate::primitives::color::ColorU8;

/// Geometry of a linear gradient: colour runs along an axis at `angle`
/// radians (0 = →, π/2 = ↓). Object-space — the gradient spans the brush
/// owner's bounding rect end-to-end at that angle.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct LinearGeometry {
    pub angle: f32,
}

/// Linear gradient — see [`LinearGeometry`] for the axis convention.
pub type LinearGradient = Gradient<LinearGeometry>;

/// Authoring builder for a [`LinearGradient`].
pub type LinearGradientBuilder = GradientBuilder<LinearGeometry>;

impl GradientGeometry for LinearGeometry {
    const DEFAULT_INTERP: Interp = Interp::Oklab;

    /// `dir = (cos(angle), sin(angle))`; the shader projects each
    /// fragment's 0..1 object-local position onto `dir`, then maps the
    /// dot product through `(t0, t1)` to the LUT row.
    ///
    /// `(t0, t1)` is always `(0, 1)` over the raw `(cos, sin)` axis, so
    /// a diagonal gradient projects to a sub-1.0 range and relies on
    /// `Spread::Pad` to clamp. That is not CSS's corner-to-corner
    /// scaling.
    fn axis_lanes(&self) -> [f32; 4] {
        let (sin, cos) = self.angle.sin_cos();
        [cos, sin, 0.0, 1.0]
    }

    fn hash_geometry<H: std::hash::Hasher>(&self, state: &mut H) {
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

    /// 2-stop shorthand — `c0` at offset 0, `c1` at offset 1. Covers
    /// the dominant UI-gradient pattern (panel chrome, button
    /// surfaces, headers).
    pub fn two_stop(angle: f32, c0: impl Into<ColorU8>, c1: impl Into<ColorU8>) -> Self {
        Self::new(angle, [Stop::new(0.0, c0), Stop::new(1.0, c1)])
    }
}
