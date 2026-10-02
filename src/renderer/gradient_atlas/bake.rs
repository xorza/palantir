//! Gradient-stop interpolation into one linear-f16 LUT row.
//!
//! Texels are premultiplied, and so is the interpolation between stops —
//! CSS Color 4 §12.3: red to transparent passes through half-red at half
//! alpha, not a darker or bluer colour from the transparent stop's hue.
//! The shaders filter between texels in the same space.

use crate::animation::animatable::Animatable;
use crate::primitives::approx;
use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::brush::gradient::stops::{GradientStops, MAX_STOPS};
use crate::primitives::color::RgbaF32;
use crate::primitives::color::oklab;
use crate::primitives::color::rgba_f16::RgbaF16;

pub(crate) const LUT_ROW_TEXELS: usize = 256;
pub(crate) type LutRowTexels = [RgbaF16; LUT_ROW_TEXELS];

/// Bake `ramp` into one row of texels.
pub(crate) fn row(ramp: &ColorRamp, out: &mut LutRowTexels) {
    let ColorRamp { stops, interp } = ramp;
    let interp = *interp;
    // No sort here: `GradientStops` holds its stops in ascending offset
    // order as a type invariant, precisely so the value that keys this
    // row and the row it bakes cannot disagree.
    let count = stops.len();
    debug_assert!(stops.is_ascending(), "GradientStops must arrive sorted");

    let mut linear_stops = [RgbaF32::TRANSPARENT; MAX_STOPS];
    for index in 0..count {
        linear_stops[index] = stops[index].color().premultiplied();
    }
    let mut oklab_stops = [[0.0; 3]; MAX_STOPS];
    // Only the Oklab ramp reads these, and an empty slice is what says so:
    // a linear bake neither computes nor carries a second colour space.
    let oklab: &[[f32; 3]] = match interp {
        Interp::Oklab => {
            // From the straight colour: a premultiplied one has no hue
            // left to convert where its alpha is zero.
            for index in 0..count {
                let color = stops[index].color();
                oklab_stops[index] = oklab::from_linear(color.r, color.g, color.b);
            }
            &oklab_stops[..count]
        }
        Interp::Linear => &[],
    };

    for (texel, color) in out.iter_mut().zip(RampTexels::new(
        stops,
        &linear_stops[..count],
        oklab,
        interp,
    )) {
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
struct RampTexels<'a> {
    stops: &'a GradientStops,
    /// The stop colours, premultiplied.
    linear: &'a [RgbaF32],
    /// Oklab coordinates of the straight stop colours, empty under
    /// [`Interp::Linear`].
    oklab: &'a [[f32; 3]],
    interp: Interp,
    /// Index of the segment's upper stop — the invariant is
    /// `stops[upper - 1].offset() <= t`, restored by [`Self::color_at`].
    upper: usize,
    /// Texel [`Iterator::next`] yields, and so the `t` it evaluates at.
    texel: usize,
}

impl<'a> RampTexels<'a> {
    /// Seat the cursor on the first segment. [`GradientStops`] holds at
    /// least two entries by construction, which is what makes that
    /// segment exist.
    fn new(
        stops: &'a GradientStops,
        linear: &'a [RgbaF32],
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
    fn color_at(&mut self, t: f32) -> RgbaF32 {
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
            Interp::Linear => RgbaF32::lerp(lower, upper_color, amount),
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

/// Interpolate two premultiplied stops in Oklab, premultiplied there too:
/// each stop's Oklab coordinates weigh by its alpha, and the blend divides
/// the interpolated alpha back out before converting.
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
    use crate::primitives::approx::internals::assert_close;
    use crate::primitives::brush::gradient::Interp;
    use crate::primitives::brush::gradient::color_ramp::ColorRamp;
    use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
    use crate::primitives::color::RgbaF32;
    use crate::primitives::color::rgba_f16::RgbaF16;
    use crate::renderer::gradient_atlas::bake::{LUT_ROW_TEXELS, RampTexels};

    /// The bake `zip`s the ramp against a fixed-length row, so a ramp
    /// that yielded fewer texels would leave the tail of the row at
    /// whatever the previous gradient baked — a silent bleed between two
    /// unrelated LUT rows. Pin the count and the reported length
    /// together: `zip` reads `size_hint`, so a wrong hint truncates just
    /// as badly as a wrong count.
    #[test]
    fn a_ramp_yields_exactly_one_row_of_texels() {
        let stops = GradientStops::new([
            Stop::new(0.0, RgbaF32::BLACK),
            Stop::new(1.0, RgbaF32::WHITE),
        ]);
        let linear = [RgbaF32::BLACK, RgbaF32::WHITE];
        let ramp = RampTexels::new(&stops, &linear, &[], Interp::Linear);

        assert_eq!(ramp.len(), LUT_ROW_TEXELS);
        assert_eq!(ramp.count(), LUT_ROW_TEXELS);
    }

    /// Opaque red to transparent blue, baked premultiplied. Texel 51 sits
    /// at t = 51/255 = 0.2: red at 0.8 alpha, `(0.8, 0, 0, 0.8)` — the
    /// transparent stop adds no blue, where a straight lerp gave
    /// `(0.8, 0, 0.2, 0.8)`. Oklab weighs each stop's coordinates by its
    /// alpha the same way, so its texel is the same red, up to the
    /// round trip through Oklab in the empty channels.
    ///
    /// A hard stop at 0.5 — stored as 128/255, texel 128's own `t`, which
    /// takes the first stop's colour — bakes texel 128 as opaque red and
    /// texel 129 as all zeros, so the bilinear filter between them is
    /// `(0.5, 0, 0, 0.5)`: red at half alpha, not a purple band.
    #[test]
    fn the_ramp_is_baked_premultiplied() {
        let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
        let clear_blue = RgbaF32::new(0.0, 0.0, 1.0, 0.0);
        let bake = |stops: &GradientStops, interp| {
            let mut row = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
            super::row(
                &ColorRamp {
                    stops: *stops,
                    interp,
                },
                &mut row,
            );
            row
        };
        let fade = GradientStops::new([Stop::new(0.0, red), Stop::new(1.0, clear_blue)]);
        // 0.8 stores as the f16 1638 × 2^-11.
        let stored = 1638.0 / 2048.0;
        let linear = RgbaF32::from(bake(&fade, Interp::Linear)[51]);
        assert_eq!(linear, RgbaF32::new(stored, 0.0, 0.0, stored));
        let oklab = RgbaF32::from(bake(&fade, Interp::Oklab)[51]);
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
        let row = bake(&hard, Interp::Linear);
        assert_eq!(row[128], RgbaF16::from(red));
        assert_eq!(row[129], RgbaF16::TRANSPARENT);
    }
}
