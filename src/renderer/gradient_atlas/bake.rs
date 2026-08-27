//! Gradient-stop interpolation into one linear-f16 LUT row.

use crate::animation::animatable::Animatable;
use crate::primitives::approx;
use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::stops::{GradientStops, MAX_STOPS};
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

    for (texel, color) in
        out.iter_mut()
            .zip(Ramp::new(stops, &linear_stops[..count], oklab, interp))
    {
        *texel = color;
    }
}

/// The stop list plus a cursor into it, yielding one row of texels in
/// order.
///
/// **An iterator rather than a `color_at(t)` the caller drives**, because
/// the resume-in-place search is sound only while `t` never decreases: the
/// cursor cannot walk back, and a smaller `t` would read a segment it has
/// already passed. Owning the sequence is what makes that true by
/// construction instead of by every caller happening to sweep upward. The
/// whole row then costs one pass over the stops rather than a
/// restart-from-the-first-segment per texel — the same reasoning that
/// hoists the linear decode out of this loop (see the module doc in
/// `gradient_atlas`), applied to the search.
///
/// [`GradientStops`] rather than a loose slice for the other half of the
/// contract: the walk below is bounded by the last stop's offset, which
/// bounds anything only while the stops ascend. That is the type's
/// invariant, so it arrives with the value.
#[derive(Debug)]
struct Ramp<'a> {
    stops: &'a GradientStops,
    linear: &'a [Color],
    /// Oklab coordinates of `linear`, empty under [`Interp::Linear`].
    oklab: &'a [[f32; 3]],
    interp: Interp,
    /// Index of the segment's upper stop — the invariant is
    /// `stops[upper - 1].offset() <= t`, restored by [`Self::color_at`].
    upper: usize,
    /// Texel [`Iterator::next`] yields, and so the `t` it evaluates at.
    texel: usize,
}

impl<'a> Ramp<'a> {
    /// Seat the cursor on the first segment. [`GradientStops`] holds at
    /// least two entries by construction, which is what makes that
    /// segment exist.
    fn new(
        stops: &'a GradientStops,
        linear: &'a [Color],
        oklab: &'a [[f32; 3]],
        interp: Interp,
    ) -> Self {
        Self {
            stops,
            linear,
            oklab,
            interp,
            upper: 1,
            texel: 0,
        }
    }

    /// The ramp colour at `t`. Private to [`Iterator::next`], which is the
    /// only thing that may name a `t` — see this type's doc.
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
        if approx::approx_zero(denominator) {
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

impl Iterator for Ramp<'_> {
    type Item = ColorF16;

    fn next(&mut self) -> Option<ColorF16> {
        let texel = self.texel;
        if texel == LUT_ROW_TEXELS {
            return None;
        }
        self.texel += 1;
        let t = texel as f32 / (LUT_ROW_TEXELS - 1) as f32;
        Some(ColorF16::from(self.color_at(t)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = LUT_ROW_TEXELS - self.texel;
        (left, Some(left))
    }
}

impl ExactSizeIterator for Ramp<'_> {}

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

#[cfg(test)]
mod tests {
    use crate::primitives::brush::gradient::Interp;
    use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
    use crate::primitives::color::{Color, ColorU8};
    use crate::renderer::gradient_atlas::bake::{LUT_ROW_TEXELS, Ramp};

    /// The bake `zip`s the ramp against a fixed-length row, so a ramp
    /// that yielded fewer texels would leave the tail of the row at
    /// whatever the previous gradient baked — a silent bleed between two
    /// unrelated LUT rows. Pin the count and the reported length
    /// together: `zip` reads `size_hint`, so a wrong hint truncates just
    /// as badly as a wrong count.
    #[test]
    fn a_ramp_yields_exactly_one_row_of_texels() {
        let stops = GradientStops::new([
            Stop::new(0.0, ColorU8::BLACK),
            Stop::new(1.0, ColorU8::WHITE),
        ]);
        let linear = [Color::BLACK, Color::WHITE];
        let ramp = Ramp::new(&stops, &linear, &[], Interp::Linear);

        assert_eq!(ramp.len(), LUT_ROW_TEXELS);
        assert_eq!(ramp.count(), LUT_ROW_TEXELS);
    }
}
