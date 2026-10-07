//! Gradient-stop interpolation into one linear-f16 LUT row. Texels and the
//! interpolation between stops are premultiplied (CSS Color 4 §12.3), as the
//! shaders filter in the same space.

use crate::animation::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::paint::brush::gradient::Interpolation;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, MAX_STOPS};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::oklab;
use crate::primitives::paint::color::rgba_f16::RgbaF16;

pub(crate) const LUT_ROW_TEXELS: usize = 256;
pub(crate) type LutRowTexels = [RgbaF16; LUT_ROW_TEXELS];

/// Bake `ramp` into one row of texels.
pub(crate) fn row(ramp: &ColorRamp, out: &mut LutRowTexels) {
    let ColorRamp {
        stops,
        interpolation,
    } = ramp;
    let interpolation = *interpolation;
    let count = stops.len();
    debug_assert!(stops.is_ascending(), "GradientStops must arrive sorted");

    let mut linear_stops = [RgbaF32::TRANSPARENT; MAX_STOPS];
    for index in 0..count {
        linear_stops[index] = stops[index].color().premultiplied();
    }
    let mut oklab_stops = [[0.0; 3]; MAX_STOPS];
    let oklab: &[[f32; 3]] = match interpolation {
        Interpolation::Oklab => {
            for index in 0..count {
                let color = stops[index].color();
                oklab_stops[index] = oklab::from_linear(color.r, color.g, color.b);
            }
            &oklab_stops[..count]
        }
        Interpolation::Linear => &[],
    };

    for (texel, color) in out.iter_mut().zip(RampTexels::new(
        stops,
        &linear_stops[..count],
        oklab,
        interpolation,
    )) {
        *texel = color;
    }
}

/// The stop list plus a cursor, yielding one row of texels in order. An
/// iterator because the resume-in-place search needs `t` never to decrease.
#[derive(Debug)]
struct RampTexels<'a> {
    stops: &'a GradientStops,
    linear: &'a [RgbaF32],
    /// Oklab coordinates of the straight stop colours; empty under [`Interpolation::Linear`].
    oklab: &'a [[f32; 3]],
    interpolation: Interpolation,
    /// Index of the segment's upper stop.
    upper: usize,
    texel: usize,
}

impl<'a> RampTexels<'a> {
    const fn new(
        stops: &'a GradientStops,
        linear: &'a [RgbaF32],
        oklab: &'a [[f32; 3]],
        interpolation: Interpolation,
    ) -> Self {
        Self {
            stops,
            linear,
            oklab,
            interpolation,
            upper: 1,
            texel: 0,
        }
    }

    fn color_at(&mut self, t: f32) -> RgbaF32 {
        if t <= self.stops[0].offset() {
            return self.linear[0];
        }
        let last = self.stops.len() - 1;
        if t >= self.stops[last].offset() {
            return self.linear[last];
        }
        while self.stops[self.upper].offset() < t {
            self.upper += 1;
        }
        let upper = self.upper;
        let lower_offset = self.stops[upper - 1].offset();
        let upper_offset = self.stops[upper].offset();
        let denominator = upper_offset - lower_offset;
        if domain::is_approx_zero(denominator) {
            return self.linear[upper];
        }
        let amount = (t - lower_offset) / denominator;
        let lower = self.linear[upper - 1];
        let upper_color = self.linear[upper];
        match self.interpolation {
            Interpolation::Linear => RgbaF32::lerp(lower, upper_color, amount),
            Interpolation::Oklab => lerp_oklab(
                lower,
                upper_color,
                self.oklab[upper - 1],
                self.oklab[upper],
                amount,
            ),
        }
    }
}

impl Iterator for RampTexels<'_> {
    type Item = RgbaF16;

    fn next(&mut self) -> Option<RgbaF16> {
        let texel = self.texel;
        if texel == LUT_ROW_TEXELS {
            return None;
        }
        self.texel += 1;
        let t = texel as f32 / (LUT_ROW_TEXELS - 1) as f32;
        Some(RgbaF16::from(self.color_at(t)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = LUT_ROW_TEXELS - self.texel;
        (left, Some(left))
    }
}

impl ExactSizeIterator for RampTexels<'_> {}

/// Interpolates two premultiplied stops in Oklab, weighing coordinates by alpha.
fn lerp_oklab(
    lower: RgbaF32,
    upper: RgbaF32,
    lower_lab: [f32; 3],
    upper_lab: [f32; 3],
    amount: f32,
) -> RgbaF32 {
    let a = <f32 as Animatable>::lerp(lower.a, upper.a, amount);
    if a <= 0.0 {
        return RgbaF32::TRANSPARENT;
    }
    let lab = [0, 1, 2].map(|i| {
        let (l, u) = (lower_lab[i] * lower.a, upper_lab[i] * upper.a);
        (l + (u - l) * amount) / a
    });
    let rgb = oklab::to_linear(lab);
    RgbaF32 {
        r: rgb[0] * a,
        g: rgb[1] * a,
        b: rgb[2] * a,
        a,
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::math::domain::internals::assert_close;
    use crate::primitives::paint::brush::gradient::Interpolation;
    use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
    use crate::primitives::paint::brush::gradient::stops::{GradientStops, Stop};
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::gradient_atlas::bake::{LUT_ROW_TEXELS, RampTexels};

    /// The bake `zip`s the ramp against a fixed-length row, so a short ramp or
    /// wrong `size_hint` would leave stale texels.
    #[test]
    fn a_ramp_yields_exactly_one_row_of_texels() {
        let stops = GradientStops::new([
            Stop::new(0.0, RgbaF32::BLACK),
            Stop::new(1.0, RgbaF32::WHITE),
        ]);
        let linear = [RgbaF32::BLACK, RgbaF32::WHITE];
        let ramp = RampTexels::new(&stops, &linear, &[], Interpolation::Linear);

        assert_eq!(ramp.len(), LUT_ROW_TEXELS);
        assert_eq!(ramp.count(), LUT_ROW_TEXELS);
    }

    /// Opaque red to transparent blue, premultiplied: texel 51 (t = 0.2) is
    /// `(0.8, 0, 0, 0.8)` with no blue. A hard stop at 0.5 bakes texel 128 red
    /// and 129 zeros, so the filter gives `(0.5, 0, 0, 0.5)`, not purple.
    #[test]
    fn the_ramp_is_baked_premultiplied() {
        let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
        let clear_blue = RgbaF32::new(0.0, 0.0, 1.0, 0.0);
        let bake = |stops: &GradientStops, interpolation| {
            let mut row = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
            super::row(
                &ColorRamp {
                    stops: *stops,
                    interpolation,
                },
                &mut row,
            );
            row
        };
        let fade = GradientStops::new([Stop::new(0.0, red), Stop::new(1.0, clear_blue)]);
        let stored = 1638.0 / 2048.0;
        let linear = RgbaF32::from(bake(&fade, Interpolation::Linear)[51]);
        assert_eq!(linear, RgbaF32::new(stored, 0.0, 0.0, stored));
        let oklab = RgbaF32::from(bake(&fade, Interpolation::Oklab)[51]);
        assert_eq!((oklab.r, oklab.a), (stored, stored));
        for residue in [oklab.g, oklab.b] {
            assert_close(
                residue,
                0.0,
                2f64.powi(-24),
                "the Oklab round trip leaves at most the smallest f16 \
                 subnormal in a channel that should be empty",
            );
        }

        let hard = GradientStops::new([Stop::new(0.5, red), Stop::new(0.5, clear_blue)]);
        let row = bake(&hard, Interpolation::Linear);
        assert_eq!(row[128], RgbaF16::from(red));
        assert_eq!(row[129], RgbaF16::TRANSPARENT);
    }
}
