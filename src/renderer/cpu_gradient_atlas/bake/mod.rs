//! Gradient-stop interpolation into one linear-f16 LUT row. Texels and the
//! interpolation between stops are premultiplied (CSS Color 4 §12.3), as the
//! shaders filter in the same space.

use crate::primitives::paint::brush::gradient::color_ramp::{ColorRamp, RampColors};
use crate::primitives::paint::color::rgba_f16::RgbaF16;

pub(crate) const LUT_ROW_TEXELS: usize = 256;
pub(crate) type LutRowTexels = [RgbaF16; LUT_ROW_TEXELS];

// The atlas uploads a row as 2048 bytes of `[r, g, b, a]` f16 lanes, which the shaders read.
const _: () = assert!(size_of::<RgbaF16>() == 8);
const _: () = assert!(size_of::<LutRowTexels>() == 2048);

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
mod tests;
