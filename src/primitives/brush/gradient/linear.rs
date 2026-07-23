use crate::primitives::approx::canon_bits;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::brush::gradient::{
    FillAxis, GradientBuilderCore, Interp, Spread, gradient_tag,
};
use crate::primitives::color::ColorU8;

/// Linear gradient — paints colour along an axis at `angle` radians
/// (0 = →, π/2 = ↓). Object-space: gradient spans the brush owner's
/// bounding rect end-to-end at the given angle.
///
/// Stops live inline via [`GradientStops`] so a
/// `LinearGradient` value is heap-free. Total size is 48 B.
///
/// **Not `Copy`** — the 40 B [`GradientStops`] made implicit per-frame
/// copies expensive through the recording chain; see `Brush`'s
/// comment for the auto-`Copy` audit story. `.clone()` is cheap
/// (one inline memcpy) — just explicit.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub struct LinearGradient {
    pub angle: f32,
    pub stops: GradientStops,
    pub spread: Spread,
    pub interp: Interp,
}

impl std::hash::Hash for LinearGradient {
    /// Hand-written: f32 fields (`angle`, per-stop `offset`) need
    /// canonical bit encoding via `canon_bits` so `-0.0` / `+0.0` and
    /// NaN bit patterns don't fragment cache keys. Used by command-
    /// buffer dedup; the atlas hashes `(stops, interp)` separately
    /// (variant-agnostic) in `gradient_atlas::GradientLutKey`.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            ((canon_bits(self.angle) as u64) << 32) | gradient_tag(self.spread, self.interp),
        );
        std::hash::Hash::hash(&self.stops, state);
    }
}

impl LinearGradient {
    /// Start an inline, allocation-free gradient builder.
    pub fn builder(angle: f32) -> LinearGradientBuilder {
        LinearGradientBuilder::new(angle)
    }

    /// General constructor. Asserts two through eight stops.
    pub fn new(angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            angle,
            stops: GradientStops::new(stops),
            spread: Spread::default(),
            interp: Interp::default(),
        }
    }

    /// 2-stop shorthand — `c0` at offset 0, `c1` at offset 1. Covers
    /// the dominant UI-gradient pattern (panel chrome, button
    /// surfaces, headers).
    pub fn two_stop(angle: f32, c0: impl Into<ColorU8>, c1: impl Into<ColorU8>) -> Self {
        Self::new(angle, [Stop::new(0.0, c0), Stop::new(1.0, c1)])
    }

    /// Gradient axis for the shader. `dir = (cos(angle), sin(angle))`;
    /// the shader projects each fragment's 0..1 object-local position
    /// onto `dir`, then maps the dot product through `(t0, t1)` to
    /// the LUT row.
    ///
    /// Slice 2 always emits `(t0, t1) = (0, 1)` and the raw
    /// `(cos, sin)` axis; diagonal gradients project to a sub-1.0
    /// range and rely on `Spread::Pad` to clamp. CSS-style
    /// corner-to-corner scaling is a slice 2.5 polish task.
    pub(crate) fn axis(&self) -> FillAxis {
        let (sin, cos) = self.angle.sin_cos();
        FillAxis::from_lanes(cos, sin, 0.0, 1.0)
    }
}

/// Chainable, allocation-free authoring builder for [`LinearGradient`].
///
/// Add two through eight stops. A ninth [`Self::stop`] panics immediately;
/// [`Self::build`] and implicit conversions panic if fewer than two were added.
#[derive(Clone, Debug)]
pub struct LinearGradientBuilder {
    angle: f32,
    common: GradientBuilderCore,
}

impl LinearGradientBuilder {
    fn new(angle: f32) -> Self {
        Self {
            angle,
            common: GradientBuilderCore::new(Interp::default()),
        }
    }

    /// Finish the gradient, requiring at least two stops.
    pub fn build(self) -> LinearGradient {
        LinearGradient {
            angle: self.angle,
            stops: GradientStops::new(self.common.stops),
            spread: self.common.spread,
            interp: self.common.interp,
        }
    }
}

gradient_builder_common!(LinearGradientBuilder);
gradient_common!(LinearGradient);

impl From<LinearGradientBuilder> for LinearGradient {
    fn from(builder: LinearGradientBuilder) -> Self {
        builder.build()
    }
}
