//! Gradient-stop interpolation into one linear-f16 LUT row.

use crate::animation::animatable::Animatable;
use crate::primitives::brush::{GradientStops, Interp, MAX_STOPS, Stop};
use crate::primitives::color::{Color, ColorF16, linear_to_oklab, oklab_to_linear};

pub(crate) const LUT_ROW_TEXELS: usize = 256;
pub(crate) type LutRowTexels = [ColorF16; LUT_ROW_TEXELS];

pub(crate) fn bake_stops(stops: &GradientStops, interp: Interp, out: &mut LutRowTexels) {
    let mut sorted: [Stop; MAX_STOPS] = Default::default();
    let count = stops.len();
    sorted[..count].copy_from_slice(stops);
    for index in 1..count {
        let mut current = index;
        while current > 0 && sorted[current - 1].offset() > sorted[current].offset() {
            sorted.swap(current - 1, current);
            current -= 1;
        }
    }

    let mut linear_stops = [Color::TRANSPARENT; MAX_STOPS];
    for index in 0..count {
        linear_stops[index] = sorted[index].color.into();
    }
    let mut oklab_stops = [[0.0; 3]; MAX_STOPS];
    if matches!(interp, Interp::Oklab) {
        for index in 0..count {
            let color = linear_stops[index];
            oklab_stops[index] = linear_to_oklab(color.r, color.g, color.b);
        }
    }

    for (index, texel) in out.iter_mut().enumerate() {
        let t = index as f32 / (LUT_ROW_TEXELS - 1) as f32;
        *texel = ColorF16::from(lerp_at(
            &sorted[..count],
            &linear_stops[..count],
            &oklab_stops[..count],
            t,
            interp,
        ));
    }
}

fn lerp_at(stops: &[Stop], linear: &[Color], oklab: &[[f32; 3]], t: f32, interp: Interp) -> Color {
    if t <= stops[0].offset() {
        return linear[0];
    }
    if t >= stops[stops.len() - 1].offset() {
        return linear[stops.len() - 1];
    }
    let mut upper = 1;
    while upper < stops.len() && stops[upper].offset() < t {
        upper += 1;
    }
    let lower_offset = stops[upper - 1].offset();
    let upper_offset = stops[upper].offset();
    let denominator = upper_offset - lower_offset;
    if denominator.abs() <= f32::EPSILON {
        return linear[upper];
    }
    let amount = (t - lower_offset) / denominator;
    let lower = linear[upper - 1];
    let upper_color = linear[upper];
    match interp {
        Interp::Linear => Color::lerp(lower, upper_color, amount),
        Interp::Oklab => lerp_oklab(lower, upper_color, oklab[upper - 1], oklab[upper], amount),
    }
}

fn lerp_oklab(
    lower: Color,
    upper: Color,
    lower_lab: [f32; 3],
    upper_lab: [f32; 3],
    amount: f32,
) -> Color {
    let lab = [
        lower_lab[0] + (upper_lab[0] - lower_lab[0]) * amount,
        lower_lab[1] + (upper_lab[1] - lower_lab[1]) * amount,
        lower_lab[2] + (upper_lab[2] - lower_lab[2]) * amount,
    ];
    let rgb = oklab_to_linear(lab);
    Color {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        a: <f32 as Animatable>::lerp(lower.a, upper.a, amount),
    }
}
