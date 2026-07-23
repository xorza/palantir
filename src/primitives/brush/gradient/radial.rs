use crate::primitives::approx::canon_bits;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::brush::gradient::{
    FillAxis, GradientBuilderCore, Interp, Spread, gradient_tag,
};
use crate::primitives::color::ColorU8;
use glam::Vec2;

/// Radial gradient — paints colour outward from `center` along the
/// elliptical radius `radius`. Object-space: both `center` and `radius`
/// are in 0..1 coordinates (origin top-left, (1,1) bottom-right of the
/// brush owner). The shader projects each fragment to
/// `t = length((local01 - center) / radius)`, applies `Spread`, and
/// samples the LUT.
///
/// Per-variant `Interp` default: `Oklab` — radial fills are usually
/// soft glows where perceptual smoothness matters most.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct RadialGradient {
    pub center: Vec2,
    pub radius: Vec2,
    pub stops: GradientStops,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for RadialGradient {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            ((canon_bits(self.center.x) as u64) << 32) | (canon_bits(self.center.y) as u64),
        );
        state.write_u64(
            ((canon_bits(self.radius.x) as u64) << 32) | (canon_bits(self.radius.y) as u64),
        );
        state.write_u64(gradient_tag(self.spread, self.interp));
        std::hash::Hash::hash(&self.stops, state);
    }
}

impl RadialGradient {
    /// Start an inline, allocation-free gradient builder.
    pub fn builder(center: Vec2, radius: Vec2) -> RadialGradientBuilder {
        RadialGradientBuilder::new(center, radius)
    }

    pub fn new(center: Vec2, radius: Vec2, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            center,
            radius,
            stops: GradientStops::new(stops),
            spread: Spread::default(),
            interp: Interp::Oklab,
        }
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

    /// Pack `(center, radius)` into a `FillAxis` wire slot. The shader
    /// reads it as `(cx, cy, rx, ry)` for the radial branch.
    pub(crate) fn axis(&self) -> FillAxis {
        FillAxis::from_lanes(self.center.x, self.center.y, self.radius.x, self.radius.y)
    }
}

/// Chainable, allocation-free authoring builder for [`RadialGradient`].
///
/// Add two through eight stops. A ninth [`Self::stop`] panics immediately;
/// [`Self::build`] and implicit conversions panic if fewer than two were added.
#[derive(Clone, Debug)]
pub struct RadialGradientBuilder {
    center: Vec2,
    radius: Vec2,
    common: GradientBuilderCore,
}

impl RadialGradientBuilder {
    fn new(center: Vec2, radius: Vec2) -> Self {
        Self {
            center,
            radius,
            common: GradientBuilderCore::new(Interp::Oklab),
        }
    }

    /// Finish the gradient, requiring at least two stops.
    pub fn build(self) -> RadialGradient {
        RadialGradient {
            center: self.center,
            radius: self.radius,
            stops: GradientStops::new(self.common.stops),
            spread: self.common.spread,
            interp: self.common.interp,
        }
    }
}

gradient_builder_common!(RadialGradientBuilder);
gradient_common!(RadialGradient);

impl From<RadialGradientBuilder> for RadialGradient {
    fn from(builder: RadialGradientBuilder) -> Self {
        builder.build()
    }
}
