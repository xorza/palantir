//! Chained, allocation-free gradient authoring.

use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::stops::{GradientStopsBuilder, Stop};
use crate::primitives::paint::brush::gradient::{
    Gradient, GradientGeometry, Interpolation, Spread,
};
use crate::primitives::paint::color::RgbaF32;

/// Chainable, allocation-free builder for [`Gradient`]: two through eight stops (a ninth [`Self::stop`] panics, as do [`Self::build`] and conversions with fewer than two). Setters are bare; the finished [`Gradient`] spells them `with_*`.
#[derive(Clone, Debug)]
#[must_use]
pub struct GradientBuilder<G> {
    geometry: G,
    stops: GradientStopsBuilder,
    spread: Spread,
    interpolation: Interpolation,
}

impl<G: GradientGeometry> GradientBuilder<G> {
    pub(super) fn new(geometry: G) -> Self {
        Self {
            geometry,
            stops: GradientStopsBuilder::default(),
            spread: Spread::default(),
            interpolation: G::DEFAULT_INTERPOLATION,
        }
    }

    /// Add a color stop at `offset`, clamped to the 0..=1 gradient range.
    #[track_caller]
    pub fn stop(mut self, offset: f32, color: RgbaF32) -> Self {
        self.stops.push(Stop::new(offset, color));
        self
    }

    /// How the gradient repeats outside 0..1.
    pub const fn spread(mut self, spread: Spread) -> Self {
        self.spread = spread;
        self
    }

    /// The colour space interpolation runs in.
    pub const fn interpolation(mut self, interpolation: Interpolation) -> Self {
        self.interpolation = interpolation;
        self
    }

    /// Finish the gradient, requiring at least two stops.
    pub fn build(self) -> Gradient<G> {
        Gradient {
            geometry: self.geometry,
            ramp: ColorRamp {
                stops: self.stops.build(),
                interpolation: self.interpolation,
            },
            spread: self.spread,
        }
    }
}

impl<G: GradientGeometry> From<GradientBuilder<G>> for Gradient<G> {
    fn from(builder: GradientBuilder<G>) -> Self {
        builder.build()
    }
}
