//! The color half of a gradient: what color each value in 0..1 maps to, independent of geometry.

use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::paint::brush::gradient::Interpolation;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, MAX_STOPS, Stop};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::oklab::Oklab;

/// A color for every value in `0..=1`: stops plus the interpolation space.
///
/// A [`Gradient`](crate::Gradient) is a ramp plus a geometry; a stroked curve's own parameter runs 0 to 1, so it takes a bare ramp ([`CurveShape::ramp`](crate::widget::CurveShape::ramp)). Also the identity of a baked atlas row: equal ramps bake equal texels.
// `repr(C)` keeps `interpolation` last; see the note on `Gradient`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct ColorRamp {
    /// Two through eight stops, in ascending offset order.
    pub stops: GradientStops,
    /// Which space the color between two stops interpolates in.
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

    /// The straight color `t` maps to, as the ramp interpolates it, before the bake rounds it
    /// to f16; past either end stop, that stop's color. A curve split into pieces paints its
    /// ramp through them with the colors at its split points.
    pub fn color_at(&self, t: f32) -> RgbaF32 {
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

/// A ramp's stops in the forms its interpolation reads: premultiplied linear, and under
/// [`Interpolation::Oklab`] the straight colors' Oklab coordinates.
#[derive(Debug)]
pub(crate) struct RampColors<'a> {
    stops: &'a GradientStops,
    interpolation: Interpolation,
    linear: [RgbaF32; MAX_STOPS],
    /// Unread under [`Interpolation::Linear`].
    oklab: [Oklab; MAX_STOPS],
}

impl<'a> RampColors<'a> {
    pub(crate) fn of(ramp: &'a ColorRamp) -> Self {
        let ColorRamp {
            stops,
            interpolation,
        } = ramp;
        let mut linear = [RgbaF32::TRANSPARENT; MAX_STOPS];
        let mut oklab = [Oklab {
            l: 0.0,
            a: 0.0,
            b: 0.0,
        }; MAX_STOPS];
        for idx in 0..stops.len() {
            let color = stops[idx].color();
            linear[idx] = color.premultiplied();
            if *interpolation == Interpolation::Oklab {
                oklab[idx] = Oklab::from_color(color);
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

    /// Interpolates the premultiplied stops `upper - 1` and `upper` in Oklab,
    /// weighing coordinates by alpha.
    fn lerp_oklab(&self, upper: usize, amount: f32) -> RgbaF32 {
        let (lower, upper_color) = (self.linear[upper - 1], self.linear[upper]);
        let (lower_lab, upper_lab) = (self.oklab[upper - 1], self.oklab[upper]);
        let a = <f32 as Animatable>::lerp(lower.a, upper_color.a, amount);
        if a <= 0.0 {
            return RgbaF32::TRANSPARENT;
        }
        lower_lab
            .scale(lower.a)
            .lerp(upper_lab.scale(upper_color.a), amount)
            .divide(a)
            .to_color()
            .with_alpha(a)
            .premultiplied()
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::brush::gradient::Interpolation;
    use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
    use crate::primitives::paint::brush::gradient::stops::Stop;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::gradient_atlas::bake::{self, LUT_ROW_TEXELS};

    /// Black to white in linear light is `0.25` grey at a quarter; opaque red to clear blue at
    /// `0.2` is premultiplied `(0.8, 0, 0, 0.8)`, which is straight red at alpha `0.8`; and past
    /// either end the end's color. Under Oklab, the color at each texel's `t`, premultiplied,
    /// is the texel the bake writes, so a curve split at `t` paints what the whole one does.
    #[test]
    fn a_ramps_color_at_t_is_what_its_bake_holds() {
        let linear = ColorRamp::two_stop(RgbaF32::BLACK, RgbaF32::WHITE)
            .with_interpolation(Interpolation::Linear);
        assert_eq!(linear.color_at(0.25), RgbaF32::new(0.25, 0.25, 0.25, 1.0));
        assert_eq!(linear.color_at(-1.0), RgbaF32::BLACK);
        assert_eq!(linear.color_at(2.0), RgbaF32::WHITE);
        let fade = ColorRamp::two_stop(
            RgbaF32::new(1.0, 0.0, 0.0, 1.0),
            RgbaF32::new(0.0, 0.0, 1.0, 0.0),
        )
        .with_interpolation(Interpolation::Linear);
        assert_eq!(fade.color_at(0.2), RgbaF32::new(1.0, 0.0, 0.0, 0.8));

        let oklab = ColorRamp::new([
            Stop::new(0.0, RgbaF32::hex(0xfa993b)),
            Stop::new(0.5, RgbaF32::hex(0x44b5ff)),
            Stop::new(1.0, RgbaF32::hex(0xbf5459).with_alpha(0.5)),
        ]);
        let mut row = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
        bake::row(&oklab, &mut row);
        for (texel, baked) in row.iter().enumerate() {
            let t = texel as f32 / (LUT_ROW_TEXELS - 1) as f32;
            assert_eq!(
                oklab.color_at(t).premultiplied().to_f16(),
                *baked,
                "texel {texel}"
            );
        }
    }
}
