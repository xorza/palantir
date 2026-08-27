use crate::primitives::approx::FloatHash;
use crate::primitives::brush::gradient::gradient_builder::GradientBuilder;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::brush::gradient::{Gradient, GradientGeometry, Interp};
use crate::primitives::color::ColorU8;
use crate::primitives::nan::NanCheck;
use glam::Vec2;

/// Geometry of a radial gradient: colour runs outward from `center`
/// along the elliptical radius `radius`. Both are object-space 0..1
/// coordinates (origin top-left, (1,1) bottom-right of the brush owner).
/// The shader projects each fragment to
/// `t = length((local01 - center) / radius)`, applies `Spread`, and
/// samples the LUT.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct RadialGeometry {
    pub center: Vec2,
    pub radius: Vec2,
}

/// Radial gradient — see [`RadialGeometry`] for the projection.
pub type RadialGradient = Gradient<RadialGeometry>;

/// Authoring builder for a [`RadialGradient`].
pub type RadialGradientBuilder = GradientBuilder<RadialGeometry>;

impl GradientGeometry for RadialGeometry {
    /// Radial fills are usually soft glows, where perceptual smoothness
    /// matters most.
    const DEFAULT_INTERP: Interp = Interp::Oklab;

    /// The shader reads these as `(cx, cy, rx, ry)` on the radial
    /// branch.
    fn axis_lanes(&self) -> [f32; 4] {
        [self.center.x, self.center.y, self.radius.x, self.radius.y]
    }

    fn hash_geometry<H: std::hash::Hasher>(&self, state: &mut H) {
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
    pub fn two_stop_centered(c0: impl Into<ColorU8>, c1: impl Into<ColorU8>) -> Self {
        Self::new(
            Vec2::splat(0.5),
            Vec2::splat(0.5),
            [Stop::new(0.0, c0), Stop::new(1.0, c1)],
        )
    }
}
