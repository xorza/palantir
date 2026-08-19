//! Gradient-stop interpolation into one linear-f16 LUT row.

use crate::animation::animatable::Animatable;
use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::stops::{GradientStops, MAX_STOPS, Stop};
use crate::primitives::color::{Color, ColorF16, linear_to_oklab, oklab_to_linear};

pub(crate) const LUT_ROW_TEXELS: usize = 256;
pub(crate) type LutRowTexels = [ColorF16; LUT_ROW_TEXELS];

pub(crate) fn bake_stops(stops: &GradientStops, interp: Interp, out: &mut LutRowTexels) {
    // No sort here: `GradientStops` holds its stops in ascending offset
    // order as a type invariant, precisely so the value that keys this
    // row and the row it bakes cannot disagree.
    let count = stops.len();
    debug_assert!(
        stops.windows(2).all(|w| w[0].offset_u8 <= w[1].offset_u8),
        "GradientStops must arrive sorted",
    );

    let mut linear_stops = [Color::TRANSPARENT; MAX_STOPS];
    for index in 0..count {
        linear_stops[index] = stops[index].color.into();
    }
    let mut oklab_stops = [[0.0; 3]; MAX_STOPS];
    // Only the Oklab ramp reads these, and an empty slice is what says so:
    // a linear bake neither computes nor carries a second colour space.
    let oklab: &[[f32; 3]] = match interp {
        Interp::Oklab => {
            for index in 0..count {
                let color = linear_stops[index];
                oklab_stops[index] = linear_to_oklab(color.r, color.g, color.b);
            }
            &oklab_stops[..count]
        }
        Interp::Linear => &[],
    };

    let mut ramp = Ramp::new(&stops[..count], &linear_stops[..count], oklab, interp);
    for (index, texel) in out.iter_mut().enumerate() {
        let t = index as f32 / (LUT_ROW_TEXELS - 1) as f32;
        *texel = ColorF16::from(ramp.color_at(t));
    }
}

/// The stop list plus a cursor into it, evaluated at a `t` that only ever
/// increases.
///
/// That monotonicity is the point: the row walks `t` from 0 to 1, so the
/// segment holding it never moves backwards and the search resumes where
/// the last texel left it. The whole row costs one pass over the stops
/// instead of a restart-from-the-first-segment per texel — the same
/// reasoning that hoists the linear decode out of this loop (see the
/// module doc in `gradient_atlas`), applied to the search.
#[derive(Debug)]
struct Ramp<'a> {
    stops: &'a [Stop],
    linear: &'a [Color],
    /// Oklab coordinates of `linear`, empty under [`Interp::Linear`].
    oklab: &'a [[f32; 3]],
    interp: Interp,
    /// Index of the segment's upper stop — the invariant is
    /// `stops[upper - 1].offset() <= t`, restored by `color_at`.
    upper: usize,
}

impl<'a> Ramp<'a> {
    /// Seat the cursor on the first segment. `stops` must hold at least two
    /// entries, which [`GradientStops`] guarantees by construction.
    fn new(stops: &'a [Stop], linear: &'a [Color], oklab: &'a [[f32; 3]], interp: Interp) -> Self {
        Self {
            stops,
            linear,
            oklab,
            interp,
            upper: 1,
        }
    }

    /// The ramp colour at `t`. **Callers must pass a non-decreasing `t`**;
    /// the cursor cannot walk back, and a smaller `t` would read the
    /// segment it has already passed.
    fn color_at(&mut self, t: f32) -> Color {
        if t <= self.stops[0].offset() {
            return self.linear[0];
        }
        let last = self.stops.len() - 1;
        if t >= self.stops[last].offset() {
            return self.linear[last];
        }
        // `t` is inside the ramp, so `stops[last].offset() > t` bounds this
        // walk before it can run off the end.
        while self.stops[self.upper].offset() < t {
            self.upper += 1;
        }
        let upper = self.upper;
        let lower_offset = self.stops[upper - 1].offset();
        let upper_offset = self.stops[upper].offset();
        let denominator = upper_offset - lower_offset;
        if denominator.abs() <= f32::EPSILON {
            return self.linear[upper];
        }
        let amount = (t - lower_offset) / denominator;
        let lower = self.linear[upper - 1];
        let upper_color = self.linear[upper];
        match self.interp {
            Interp::Linear => Color::lerp(lower, upper_color, amount),
            Interp::Oklab => lerp_oklab(
                lower,
                upper_color,
                self.oklab[upper - 1],
                self.oklab[upper],
                amount,
            ),
        }
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
