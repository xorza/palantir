use crate::primitives::approx::FloatHash;
use crate::primitives::brush::gradient::gradient_builder::GradientBuilder;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::brush::gradient::{Gradient, GradientGeometry, Interp};
use crate::primitives::color::ColorU8;
use crate::primitives::nan::NanCheck;
use glam::Vec2;

/// Geometry of a conic (sweep) gradient: the parametric axis 0..1 sweeps
/// around `center` starting at `start_angle` radians, counter-clockwise.
/// Object-space `center` is in 0..1 coordinates. The shader projects each
/// fragment to `t = fract((atan2(dy, dx) - start_angle) / TAU + 1.0)`,
/// applies `Spread`, samples the LUT.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct ConicGeometry {
    pub center: Vec2,
    pub start_angle: f32,
}

/// Conic (sweep) gradient — see [`ConicGeometry`] for the sweep.
pub type ConicGradient = Gradient<ConicGeometry>;

/// Authoring builder for a [`ConicGradient`].
pub type ConicGradientBuilder = GradientBuilder<ConicGeometry>;

impl GradientGeometry for ConicGeometry {
    /// Conic gradients commonly implement colour-wheel / hue-rotation
    /// visuals where straight linear-RGB interpolation gives the most
    /// predictable hue sweep; Oklab can shift the perceived hue at the
    /// midpoint. (A future `Oklch{hue}` interp would be the truly right
    /// default.)
    const DEFAULT_INTERP: Interp = Interp::Linear;

    /// The shader reads these as `(cx, cy, start_angle, _)` on the conic
    /// branch.
    fn axis_lanes(&self) -> [f32; 4] {
        [self.center.x, self.center.y, self.start_angle, 0.0]
    }

    fn hash_geometry<H: std::hash::Hasher>(&self, state: &mut H) {
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

    /// Centred shorthand — `center = (0.5, 0.5)`, starts at angle 0
    /// (positive x-axis, sweeping CCW). 2 stops at offsets 0/1.
    pub fn two_stop_centered(c0: impl Into<ColorU8>, c1: impl Into<ColorU8>) -> Self {
        Self::new(
            Vec2::splat(0.5),
            0.0,
            [Stop::new(0.0, c0), Stop::new(1.0, c1)],
        )
    }
}
