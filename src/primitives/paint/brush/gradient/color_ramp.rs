//! The colour half of a gradient: what colour each value in 0..1 maps to, independent of geometry.

use crate::primitives::paint::brush::gradient::Interpolation;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::paint::color::RgbaF32;

/// A colour for every value in `0..=1`: stops plus the interpolation space.
///
/// A [`Gradient`](crate::Gradient) is a ramp plus a geometry; a stroked curve's own parameter runs 0 to 1, so it takes a bare ramp ([`CurveShape::ramp`](crate::widget::CurveShape::ramp)). Also the identity of a baked atlas row: equal ramps bake equal texels.
// `repr(C)` keeps `interpolation` last; see the note on `Gradient`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct ColorRamp {
    /// Two through eight stops, in ascending offset order.
    pub stops: GradientStops,
    /// Which space the colour between two stops interpolates in.
    pub interpolation: Interpolation,
}

impl ColorRamp {
    /// A ramp through `stops` in [`Interpolation::Oklab`]; asserts two through eight stops.
    pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            stops: GradientStops::new(stops),
            interpolation: Interpolation::default(),
        }
    }

    /// `c0` at 0 and `c1` at 1.
    pub fn two_stop(c0: RgbaF32, c1: RgbaF32) -> Self {
        Self::new([Stop::new(0.0, c0), Stop::new(1.0, c1)])
    }

    /// Override the interpolation colour space.
    pub const fn with_interpolation(mut self, interpolation: Interpolation) -> Self {
        self.interpolation = interpolation;
        self
    }

    /// Paints nothing visible when every stop is transparent.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        let stops = self.stops.as_slice();
        let mut i = 0;
        while i < stops.len() {
            if !stops[i].color().is_noop() {
                return false;
            }
            i += 1;
        }
        true
    }
}
