//! The spectrum a dispersive lens spreads, as the composite sums it back to color: the tables
//! only the GPU reads, over the ends [`LensOptics`](crate::primitives::paint::material::lens_optics) names.

use crate::primitives::math::libm_ext::Libm;
use crate::primitives::paint::material::lens_optics::{
    SPECTRUM_RED, SPECTRUM_SAMPLES, SPECTRUM_VIOLET,
};

/// The CIE 1931 2° observer at `nm`, by Wyman, Sloan and Shirley's multi-lobe fit ("Simple
/// Analytic Approximations to the CIE XYZ Color Matching Functions", JCGT 2013), as Filament's
/// specgen samples it.
fn cie_xyz(nm: f64) -> [f64; 3] {
    let lobe = |mu: f64, below: f64, above: f64| {
        let t = (nm - mu) / if nm < mu { below } else { above };
        (-0.5 * t * t).libm_exp()
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
        let u = |wavelength: f32| 1.0 / f64::from(wavelength).libm_powi(2);
        let (red, violet) = (u(SPECTRUM_RED), u(SPECTRUM_VIOLET));
        let raw: [[f64; 3]; SPECTRUM_SAMPLES] = std::array::from_fn(|j| {
            let middle = (j as f64 + 0.5) / n;
            let wavelength = (red + middle * (violet - red)).libm_powf(-0.5);
            cie_xyz(1000.0 * wavelength).map(|w| w * wavelength.libm_powi(3))
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
mod tests;
