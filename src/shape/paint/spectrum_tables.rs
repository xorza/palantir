//! The spectrum a dispersive lens spreads, as the composite sums it back to color.

/// The ends of the spectrum the composite spreads, µm: all but a sliver of the CIE 1931
/// observer's response lies between them. The index peaks at the violet end.
pub(crate) const SPECTRUM_RED: f32 = 0.7;
pub(crate) const SPECTRUM_VIOLET: f32 = 0.4;

/// The cells the spectrum's weight is held flat across: even in `1 / λ²`, as the reads step,
/// 3 nm wide at the violet end and 14 nm at the red. Each lobe of the observer's response spans
/// several: 3.7 nm cells under the narrowest (σ 11.8 nm, z̄ at 437 nm), 14 nm under x̄'s red one
/// (σ 31 nm).
pub(crate) const SPECTRUM_SAMPLES: usize = 48;

/// The CIE 1931 2° observer at `nm`, by Wyman, Sloan and Shirley's multi-lobe fit ("Simple
/// Analytic Approximations to the CIE XYZ Color Matching Functions", JCGT 2013), as Filament's
/// specgen samples it.
fn cie_xyz(nm: f64) -> [f64; 3] {
    let lobe = |mu: f64, below: f64, above: f64| {
        let t = (nm - mu) / if nm < mu { below } else { above };
        (-0.5 * t * t).exp()
    };
    [
        1.056 * lobe(599.8, 37.9, 31.0) + 0.362 * lobe(442.0, 16.0, 26.7)
            - 0.065 * lobe(501.1, 20.4, 26.2),
        0.821 * lobe(568.8, 46.9, 40.5) + 0.286 * lobe(530.9, 16.3, 31.1),
        1.217 * lobe(437.0, 11.8, 36.0) + 0.681 * lobe(459.0, 26.0, 13.8),
    ]
}

/// The spectrum the composite sums, in CIE XYZ over the share `t` of the way along it, red end
/// first: the observer's weight, as a density over `t`, held flat across each of
/// [`SPECTRUM_SAMPLES`] cells, and its running integrals at each cell's start. The composite
/// integrates any stretch of `t` from them in closed form, whatever the stretch's length.
#[derive(Debug)]
pub(crate) struct SpectrumTables {
    /// Cell `j`'s integral of the weight: the observer at the wavelength in the middle of the
    /// cell, times `λ³`, the measure `|dλ / du|` carries, as `t` runs evenly in `u = 1 / λ²`.
    /// Each channel sums to 1, as Filament's specgen normalizes it, so the sum of every
    /// wavelength's share of one color is that color.
    pub(crate) weights: [[f32; 3]; SPECTRUM_SAMPLES],
    /// `∫₀ᵗ W` at cell `j`'s start, `t = j / N`.
    pub(crate) cumulative: [[f32; 3]; SPECTRUM_SAMPLES],
    /// `∫₀ᵗ τ · W(τ) dτ` at cell `j`'s start.
    pub(crate) moments: [[f32; 3]; SPECTRUM_SAMPLES],
}

impl SpectrumTables {
    /// The tables, summed in `f64` and rounded once to `f32`.
    pub(crate) fn new() -> Self {
        let n = SPECTRUM_SAMPLES as f64;
        let u = |wavelength: f32| 1.0 / f64::from(wavelength).powi(2);
        let (red, violet) = (u(SPECTRUM_RED), u(SPECTRUM_VIOLET));
        let raw: [[f64; 3]; SPECTRUM_SAMPLES] = std::array::from_fn(|j| {
            let middle = (j as f64 + 0.5) / n;
            let wavelength = (red + middle * (violet - red)).powf(-0.5);
            cie_xyz(1000.0 * wavelength).map(|w| w * wavelength.powi(3))
        });
        let sums = raw.iter().fold([0.0; 3], |sum, w| {
            [sum[0] + w[0], sum[1] + w[1], sum[2] + w[2]]
        });
        let weights = raw.map(|w| [0, 1, 2].map(|c| w[c] / sums[c]));
        let mut cumulative = [[0.0; 3]; SPECTRUM_SAMPLES];
        let mut moments = [[0.0; 3]; SPECTRUM_SAMPLES];
        let (mut area, mut moment) = ([0.0_f64; 3], [0.0_f64; 3]);
        for (j, w) in weights.iter().enumerate() {
            cumulative[j] = area.map(|v| v as f32);
            moments[j] = moment.map(|v| v as f32);
            let middle = (j as f64 + 0.5) / n;
            for c in 0..3 {
                area[c] += w[c];
                moment[c] += w[c] * middle;
            }
        }
        Self {
            weights: weights.map(|w| w.map(|v| v as f32)),
            cumulative,
            moments,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::shape::paint::spectrum_tables::{SPECTRUM_SAMPLES, SpectrumTables, cie_xyz};

    /// Wyman's fit holds the CIE 1931 table's peaks within its stated error: ȳ at 555 nm is 1.000
    /// in the table, x̄ at 600 nm 1.0622 and z̄ at 445 nm 1.7826. The weights sum to 1 in each
    /// channel within an `f32` rounding a cell; the red end holds the most of x̄'s weight, the
    /// violet end of z̄'s, and green the most of ȳ's. The running integrals are the sums of the
    /// cells before: the area of the weight, and its moment about `t = 0` at each cell's middle.
    #[test]
    fn the_spectrum_sums_to_one_with_each_channel_where_its_lobe_peaks() {
        let [_, y, _] = cie_xyz(555.0);
        assert!((y - 1.0).abs() < 0.03, "{y}");
        let [x, _, _] = cie_xyz(600.0);
        assert!((x - 1.0622).abs() < 0.03, "{x}");
        let [_, _, z] = cie_xyz(445.0);
        assert!((z - 1.7826).abs() < 0.05, "{z}");

        let tables = SpectrumTables::new();
        let w = &tables.weights;
        let rounding = SPECTRUM_SAMPLES as f64 * f64::from(f32::EPSILON);
        for c in 0..3 {
            let sum: f64 = w.iter().map(|w| f64::from(w[c])).sum();
            assert!((sum - 1.0).abs() < rounding, "channel {c}: {sum}");
        }
        let peak = |c: usize| {
            (0..SPECTRUM_SAMPLES)
                .max_by(|&a, &b| w[a][c].total_cmp(&w[b][c]))
                .unwrap()
        };
        assert!(
            peak(0) < peak(1) && peak(1) < peak(2),
            "{}, {}, {}",
            peak(0),
            peak(1),
            peak(2)
        );

        assert_eq!(tables.cumulative[0], [0.0; 3]);
        assert_eq!(tables.moments[0], [0.0; 3]);
        let n = SPECTRUM_SAMPLES as f64;
        let (mut area, mut moment) = ([0.0_f64; 3], [0.0_f64; 3]);
        let starts = tables.cumulative.iter().zip(&tables.moments);
        for (j, (weight, (got_area, got_moment))) in w.iter().zip(starts).enumerate() {
            let sums = area.iter().zip(&moment);
            for ((want_area, want_moment), (got_area, got_moment)) in
                sums.zip(got_area.iter().zip(got_moment))
            {
                assert!((f64::from(*got_area) - want_area).abs() < rounding, "{j}");
                assert!(
                    (f64::from(*got_moment) - want_moment).abs() < rounding,
                    "{j}"
                );
            }
            let middle = (j as f64 + 0.5) / n;
            for ((area, moment), w) in area.iter_mut().zip(&mut moment).zip(weight) {
                *area += f64::from(*w);
                *moment += f64::from(*w) * middle;
            }
        }
    }
}
