//! Gradient-stop interpolation into one linear-f16 LUT row. Texels and the
//! interpolation between stops are premultiplied (CSS Color 4 §12.3), as the
//! shaders filter in the same space.

use crate::primitives::paint::brush::gradient::color_ramp::{ColorRamp, RampColors};
use crate::primitives::paint::color::rgba_f16::RgbaF16;

pub(crate) const LUT_ROW_TEXELS: usize = 256;
pub(crate) type LutRowTexels = [RgbaF16; LUT_ROW_TEXELS];

/// Bake `ramp` into one row of texels.
pub(crate) fn row(ramp: &ColorRamp, out: &mut LutRowTexels) {
    debug_assert!(
        ramp.stops.is_ascending(),
        "GradientStops must arrive sorted"
    );
    for (texel, color) in out.iter_mut().zip(RampTexels::new(ramp)) {
        *texel = color;
    }
}

/// A ramp's colors plus a cursor, yielding one row of texels in order. An
/// iterator because the resume-in-place search needs `t` never to decrease.
#[derive(Debug)]
struct RampTexels<'a> {
    colors: RampColors<'a>,
    /// Index of the segment's upper stop.
    upper: usize,
    texel: usize,
}

impl<'a> RampTexels<'a> {
    fn new(ramp: &'a ColorRamp) -> Self {
        Self {
            colors: RampColors::of(ramp),
            upper: 1,
            texel: 0,
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
        Some(self.colors.at(t, &mut self.upper).to_f16())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = LUT_ROW_TEXELS - self.texel;
        (left, Some(left))
    }
}

impl ExactSizeIterator for RampTexels<'_> {}

#[cfg(test)]
mod tests {
    use crate::primitives::math::domain::internals::assert_close;
    use crate::primitives::paint::brush::gradient::Interpolation;
    use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
    use crate::primitives::paint::brush::gradient::stops::{GradientStops, Stop};
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::gradient_atlas::bake::{self, LUT_ROW_TEXELS, RampTexels};

    /// The bake `zip`s the ramp against a fixed-length row, so a short ramp or
    /// wrong `size_hint` would leave stale texels.
    #[test]
    fn a_ramp_yields_exactly_one_row_of_texels() {
        let stops = GradientStops::new([
            Stop::new(0.0, RgbaF32::BLACK),
            Stop::new(1.0, RgbaF32::WHITE),
        ]);
        let color_ramp = ColorRamp {
            stops,
            interpolation: Interpolation::Linear,
        };
        let ramp = RampTexels::new(&color_ramp);

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
            bake::row(
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
        assert_eq!(row[128], red.to_f16());
        assert_eq!(row[129], RgbaF16::TRANSPARENT);
    }
}
