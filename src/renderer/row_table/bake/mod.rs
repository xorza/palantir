//! Gradient-stop interpolation into one linear-f16 LUT row. Texels and the
//! interpolation between stops are premultiplied (CSS Color 4 §12.3), as the
//! shaders filter in the same space.

use crate::primitives::paint::brush::gradient::color_ramp::{ColorRamp, RampColors};
use crate::primitives::paint::color::rgba_f16::RgbaF16;

pub(crate) const LUT_ROW_TEXELS: usize = 256;
pub(crate) type LutRowTexels = [RgbaF16; LUT_ROW_TEXELS];

// The table uploads a row as 2048 bytes of `[r, g, b, a]` f16 lanes, which the shaders read.
const _: () = assert!(size_of::<RgbaF16>() == 8);
const _: () = assert!(size_of::<LutRowTexels>() == 2048);

/// Bake `ramp` into one row of texels, `t` rising texel by texel, so the stop search resumes where
/// the last texel left it.
pub(crate) fn row(ramp: &ColorRamp, out: &mut LutRowTexels) {
    debug_assert!(
        ramp.stops.is_ascending(),
        "GradientStops must arrive sorted"
    );
    let colors = RampColors::of(ramp);
    let mut upper = 1;
    for (idx, texel) in out.iter_mut().enumerate() {
        let t = idx as f32 / (LUT_ROW_TEXELS - 1) as f32;
        *texel = colors.at(t, &mut upper).to_f16();
    }
}

#[cfg(test)]
mod tests;
