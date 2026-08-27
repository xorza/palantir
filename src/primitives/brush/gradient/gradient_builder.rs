use crate::primitives::brush::gradient::stops::{GradientStopsBuilder, Stop};
use crate::primitives::brush::gradient::{Gradient, GradientGeometry, Interp, Spread};
use crate::primitives::color::ColorU8;

/// Chainable, allocation-free authoring builder for [`Gradient`].
///
/// Add two through eight stops. A ninth [`Self::stop`] panics immediately;
/// [`Self::build`] and implicit conversions panic if fewer than two were
/// added.
///
/// `with_spread` / `with_interp` are spelled the same here as on the
/// finished [`Gradient`] on purpose, so a caller needn't know which side
/// of the build it is holding.
#[derive(Clone, Debug)]
pub struct GradientBuilder<G> {
    geometry: G,
    stops: GradientStopsBuilder,
    spread: Spread,
    interp: Interp,
}

impl<G: GradientGeometry> GradientBuilder<G> {
    pub(super) fn new(geometry: G) -> Self {
        Self {
            geometry,
            stops: GradientStopsBuilder::default(),
            spread: Spread::default(),
            interp: G::DEFAULT_INTERP,
        }
    }

    /// Add a color stop at `offset`, clamped to the 0..=1 gradient range.
    pub fn stop(mut self, offset: f32, color: impl Into<ColorU8>) -> Self {
        self.stops.push(Stop::new(offset, color));
        self
    }

    pub fn with_spread(mut self, spread: Spread) -> Self {
        self.spread = spread;
        self
    }

    pub fn with_interp(mut self, interp: Interp) -> Self {
        self.interp = interp;
        self
    }

    /// Finish the gradient, requiring at least two stops.
    pub fn build(self) -> Gradient<G> {
        Gradient {
            geometry: self.geometry,
            stops: self.stops.build(),
            spread: self.spread,
            interp: self.interp,
        }
    }
}

impl<G: GradientGeometry> From<GradientBuilder<G>> for Gradient<G> {
    fn from(builder: GradientBuilder<G>) -> Self {
        builder.build()
    }
}
