//! The colour half of a gradient: what colour each value from 0 to 1
//! maps to, with no idea where on a shape those values fall.

use crate::primitives::paint::brush::gradient::Interpolation;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::paint::color::RgbaF32;

/// A colour for every value in `0..=1`: stops, and the space the colour
/// between them interpolates in.
///
/// A [`Gradient`](crate::Gradient) is a ramp plus a geometry that maps
/// each point of a fill onto the ramp. A stroked curve needs no geometry:
/// its own parameter already runs from 0 at the start to 1 at the end, so
/// it takes a bare ramp — see [`CurveShape::ramp`](crate::widget::CurveShape::ramp).
///
/// Also the exact identity of a baked atlas row: two ramps that compare
/// equal bake the same texels, whatever shape paints them.
// `repr(C)` keeps `interpolation` last — see the note on `Gradient`.
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
    /// A ramp through `stops`, interpolated in [`Interpolation::Oklab`]. Asserts
    /// two through eight stops.
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

    /// Override the colour space interpolation runs in. Builder-style.
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
