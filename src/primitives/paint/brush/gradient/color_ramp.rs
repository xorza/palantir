//! The color half of a gradient: what color each value in 0..1 maps to, independent of geometry.

use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::paint::brush::gradient::Interpolation;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, MAX_STOPS, Stop};
use crate::primitives::paint::color::RgbaF32;

/// A color for every value in `0..=1`: stops plus the interpolation space.
///
/// A [`Gradient`](crate::Gradient) is a ramp plus a geometry; a stroked curve's own parameter runs 0 to 1, so it takes a bare ramp ([`Stroke::with_ramp`](crate::Stroke::with_ramp)). Also the identity of a baked atlas row: equal ramps bake equal texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[must_use]
pub struct ColorRamp {
    /// Two through eight stops, in ascending offset order.
    pub stops: GradientStops,
    /// Which space the color between two stops interpolates in.
    pub interpolation: Interpolation,
}

impl ColorRamp {
    /// A ramp through `stops` in [`Interpolation::Oklab`].
    ///
    /// # Panics
    ///
    /// As [`GradientStops::new`].
    #[track_caller]
    pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self {
        Self {
            stops: GradientStops::new(stops),
            interpolation: Interpolation::default(),
        }
    }

    /// A ramp through `colors` spread evenly from 0 to 1, as Flutter's, SwiftUI's and Skia's
    /// gradients spread colors given no positions.
    ///
    /// # Panics
    ///
    /// Panics unless there are two through eight `colors`, each a
    /// [color](crate::widget::domain::color).
    #[track_caller]
    pub fn evenly(colors: impl IntoIterator<Item = RgbaF32>) -> Self {
        let mut held = [RgbaF32::TRANSPARENT; MAX_STOPS];
        let mut len = 0;
        for color in colors {
            assert!(
                len < MAX_STOPS,
                "gradient stop count exceeds MAX_STOPS = {MAX_STOPS}"
            );
            held[len] = color;
            len += 1;
        }
        assert!(len >= 2, "gradient requires at least 2 stops, got {len}");
        let last = (len - 1) as f32;
        let mut stops = [Stop::default(); MAX_STOPS];
        // A loop, not a closure, so each stop's check reports the caller's line.
        for idx in 0..len {
            stops[idx] = Stop::new(idx as f32 / last, held[idx]);
        }
        Self::new(stops.into_iter().take(len))
    }

    /// The straight color `t` maps to, as the ramp interpolates it, before the bake rounds it
    /// to f16; past either end stop, that stop's color, and the first stop's for a NaN. A curve
    /// split into pieces paints its ramp through them with the colors at its split points.
    pub fn color_at(&self, t: f32) -> RgbaF32 {
        // A NaN passes both end tests and would blend to a NaN color.
        let t = if t.is_nan() { 0.0 } else { t };
        let premultiplied = RampColors::of(self).at(t, &mut 1);
        if premultiplied.a <= 0.0 {
            return RgbaF32::TRANSPARENT;
        }
        let a = premultiplied.a;
        RgbaF32::new(
            premultiplied.r / a,
            premultiplied.g / a,
            premultiplied.b / a,
            a,
        )
    }

    /// Override the interpolation color space.
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

/// `colors` spread evenly ([`ColorRamp::evenly`]).
impl<const N: usize> From<[RgbaF32; N]> for ColorRamp {
    #[track_caller]
    fn from(colors: [RgbaF32; N]) -> Self {
        Self::evenly(colors)
    }
}

/// `stops` where they say ([`ColorRamp::new`]).
impl<const N: usize> From<[Stop; N]> for ColorRamp {
    #[track_caller]
    fn from(stops: [Stop; N]) -> Self {
        Self::new(stops)
    }
}

/// A ramp's stops in the forms its interpolation reads: premultiplied linear, and under
/// [`Interpolation::Oklab`] premultiplied Oklab, the motion space a color animates in.
#[derive(Debug)]
pub(crate) struct RampColors<'a> {
    stops: &'a GradientStops,
    interpolation: Interpolation,
    linear: [RgbaF32; MAX_STOPS],
    /// Unread under [`Interpolation::Linear`].
    oklab: [RgbaF32; MAX_STOPS],
}

impl<'a> RampColors<'a> {
    pub(crate) fn of(ramp: &'a ColorRamp) -> Self {
        let ColorRamp {
            stops,
            interpolation,
        } = ramp;
        let mut linear = [RgbaF32::TRANSPARENT; MAX_STOPS];
        let mut oklab = [RgbaF32::TRANSPARENT; MAX_STOPS];
        for idx in 0..stops.len() {
            let color = stops[idx].color();
            linear[idx] = color.premultiplied();
            if *interpolation == Interpolation::Oklab {
                oklab[idx] = color.to_motion();
            }
        }
        Self {
            stops,
            interpolation: *interpolation,
            linear,
            oklab,
        }
    }

    /// The premultiplied color at `t`. `upper` is the index of the segment's upper stop, a
    /// cursor that resumes in place, so successive calls must not decrease `t`.
    pub(crate) fn at(&self, t: f32, upper: &mut usize) -> RgbaF32 {
        if t <= self.stops[0].offset() {
            return self.linear[0];
        }
        let last = self.stops.len() - 1;
        if t >= self.stops[last].offset() {
            return self.linear[last];
        }
        while self.stops[*upper].offset() < t {
            *upper += 1;
        }
        let upper = *upper;
        let lower_offset = self.stops[upper - 1].offset();
        let upper_offset = self.stops[upper].offset();
        let denominator = upper_offset - lower_offset;
        if domain::is_approx_zero(denominator) {
            return self.linear[upper];
        }
        let amount = (t - lower_offset) / denominator;
        match self.interpolation {
            Interpolation::Linear => self.linear[upper - 1].mix(self.linear[upper], amount),
            Interpolation::Oklab => self.lerp_oklab(upper, amount),
        }
    }

    /// Interpolates stops `upper - 1` and `upper` in premultiplied Oklab, as a color animates. The
    /// cap of `from_motion` never acts beyond rounding: a stop is 8-bit sRGB, in gamut, and a ramp
    /// does not overshoot its stops.
    fn lerp_oklab(&self, upper: usize, amount: f32) -> RgbaF32 {
        RgbaF32::from_motion(self.oklab[upper - 1].mix(self.oklab[upper], amount)).premultiplied()
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::brush::gradient::Interpolation;
    use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
    use crate::primitives::paint::color::RgbaF32;

    /// Black to white in linear light is `0.25` grey at a quarter; opaque red to clear blue at
    /// `0.2` is premultiplied `(0.8, 0, 0, 0.8)`, which is straight red at alpha `0.8`; and past
    /// either end the end's color. `row_table`'s bake tests hold each texel to it.
    #[test]
    fn a_ramps_color_at_t_blends_its_stops() {
        let linear = ColorRamp::from([RgbaF32::BLACK, RgbaF32::WHITE])
            .with_interpolation(Interpolation::Linear);
        assert_eq!(linear.color_at(0.25), RgbaF32::new(0.25, 0.25, 0.25, 1.0));
        assert_eq!(linear.color_at(-1.0), RgbaF32::BLACK);
        assert_eq!(linear.color_at(2.0), RgbaF32::WHITE);
        assert_eq!(linear.color_at(f32::NEG_INFINITY), RgbaF32::BLACK);
        assert_eq!(linear.color_at(f32::INFINITY), RgbaF32::WHITE);
        assert_eq!(linear.color_at(f32::NAN), RgbaF32::BLACK);
        let fade = ColorRamp::from([
            RgbaF32::new(1.0, 0.0, 0.0, 1.0),
            RgbaF32::new(0.0, 0.0, 1.0, 0.0),
        ])
        .with_interpolation(Interpolation::Linear);
        assert_eq!(fade.color_at(0.2), RgbaF32::new(1.0, 0.0, 0.0, 0.8));
    }
}
