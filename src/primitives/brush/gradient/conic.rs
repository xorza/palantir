use crate::primitives::approx::canon_bits;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::brush::gradient::{
    FillAxis, GradientBuilderCore, Interp, Spread, gradient_tag,
};
use crate::primitives::color::ColorU8;
use glam::Vec2;

/// Conic (sweep) gradient — paints colour by sweeping the parametric
/// axis 0..1 around `center` starting at `start_angle` radians,
/// counter-clockwise. Object-space `center` is in 0..1 coordinates.
/// The shader projects each fragment to
/// `t = fract((atan2(dy, dx) - start_angle) / TAU + 1.0)`, applies
/// `Spread`, samples the LUT.
///
/// Per-variant `Interp` default: `Linear`. Conic gradients commonly
/// implement colour-wheel / hue-rotation visuals where straight
/// linear-RGB interpolation gives the most predictable hue sweep;
/// Oklab can shift the perceived hue at the midpoint. (A future
/// `Oklch{hue}` interp would be the truly right default.)
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct ConicGradient {
    pub center: Vec2,
    pub start_angle: f32,
    pub stops: GradientStops,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for ConicGradient {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            ((canon_bits(self.center.x) as u64) << 32) | (canon_bits(self.center.y) as u64),
        );
        state.write_u64(
            ((canon_bits(self.start_angle) as u64) << 32) | gradient_tag(self.spread, self.interp),
        );
        std::hash::Hash::hash(&self.stops, state);
    }
}

impl ConicGradient {
    /// Start an inline, allocation-free gradient builder.
    pub fn builder(center: Vec2, start_angle: f32) -> ConicGradientBuilder {
        ConicGradientBuilder::new(center, start_angle)
    }

    pub fn new(center: Vec2, start_angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            center,
            start_angle,
            stops: GradientStops::new(stops),
            spread: Spread::default(),
            interp: Interp::Linear,
        }
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

    /// Pack `(center, start_angle)` into a `FillAxis` wire slot. The shader
    /// reads it as `(cx, cy, start_angle, _)` for the conic branch.
    pub(crate) fn axis(&self) -> FillAxis {
        FillAxis::from_lanes(self.center.x, self.center.y, self.start_angle, 0.0)
    }
}

/// Chainable, allocation-free authoring builder for [`ConicGradient`].
///
/// Add two through eight stops. A ninth [`Self::stop`] panics immediately;
/// [`Self::build`] and implicit conversions panic if fewer than two were added.
#[derive(Clone, Debug)]
pub struct ConicGradientBuilder {
    center: Vec2,
    start_angle: f32,
    common: GradientBuilderCore,
}

impl ConicGradientBuilder {
    fn new(center: Vec2, start_angle: f32) -> Self {
        Self {
            center,
            start_angle,
            common: GradientBuilderCore::new(Interp::Linear),
        }
    }

    /// Finish the gradient, requiring at least two stops.
    pub fn build(self) -> ConicGradient {
        ConicGradient {
            center: self.center,
            start_angle: self.start_angle,
            stops: GradientStops::new(self.common.stops),
            spread: self.common.spread,
            interp: self.common.interp,
        }
    }
}

gradient_builder_common!(ConicGradientBuilder);
gradient_common!(ConicGradient);

impl From<ConicGradientBuilder> for ConicGradient {
    fn from(builder: ConicGradientBuilder) -> Self {
        builder.build()
    }
}
