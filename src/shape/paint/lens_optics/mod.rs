//! The path light takes through a pane's lens: the closed form the composite shades with.

/// One lens in physical px: a bezel `bezel` wide rising to `thickness` as a squircle of the
/// inward fraction, of refractive index `index`. Light falls straight down onto the glass and
/// refracts once at its top surface, as kube.io's model has it; the backdrop shows where the bent
/// ray meets the plane under the glass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LensOptics {
    pub(crate) bezel: f32,
    pub(crate) thickness: f32,
    pub(crate) index: f32,
}

/// The ends of the spectrum the composite spreads, µm: all but a sliver of the CIE 1931
/// observer's response lies between them. The index peaks at the violet end.
pub(crate) const SPECTRUM_RED: f32 = 0.7;
pub(crate) const SPECTRUM_VIOLET: f32 = 0.4;

/// Fraunhofer's D, F and C lines, µm: a lens's index is its index at D, and its dispersion the
/// step from C to F, `n_F − n_C`, as glass catalogs give it beside the Abbe number.
pub(crate) const LINE_D: f32 = 0.5893;
pub(crate) const LINE_F: f32 = 0.4861;
pub(crate) const LINE_C: f32 = 0.6563;

/// The share of `n_F − n_C` by which the index at `wavelength` µm passes its index at D: Cauchy's
/// `n(λ) = A + B / λ²` through both, `(λ⁻² − λ_D⁻²) / (λ_F⁻² − λ_C⁻²)`, as Filament's specgen
/// offsets it.
const fn cauchy_offset(wavelength: f32) -> f32 {
    (1.0 / (wavelength * wavelength) - 1.0 / (LINE_D * LINE_D))
        / (1.0 / (LINE_F * LINE_F) - 1.0 / (LINE_C * LINE_C))
}

impl LensOptics {
    /// The densest index a lens of `index` at D and `dispersion` (`n_F − n_C`) spreads light to,
    /// at the spectrum's violet end: the one whose sample goes furthest.
    pub(crate) const fn violet_index(index: f32, dispersion: f32) -> f32 {
        index + dispersion * cauchy_offset(SPECTRUM_VIOLET)
    }

    /// A bound on how far any part of the bezel sends its sample, in px, at index `index`: the
    /// glass is no higher than `thickness`, and the ray's lean `θ₁ − asin(sin θ₁ / n)` grows with
    /// the surface's tilt `θ₁` toward its limit at a vertical edge, `90° − asin(1/n)`, whose
    /// tangent is `√(n² − 1)`. Rounded up a pixel for the composite's bilinear read, and
    /// saturated at `u32::MAX`.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a length times a square root is not negative"
    )]
    pub(crate) fn reach(self, index: f32) -> u32 {
        (self.lean(index).ceil() as u32).saturating_add(1)
    }

    /// How far past a rounded-rect pane whose narrowest side is `narrowest` px the bezel sends a
    /// sample, at index `index`: a sample `d` px in moves inward by at most the lean, along the
    /// edge's normal, and a rounded rect's chord along an inward normal is no shorter than its
    /// narrowest side, so the sample leaves the pane by at most `d + lean − narrowest`, with `d`
    /// no deeper than the bezel or half the narrowest side. A pixel more, as the composite takes
    /// the normal from the distance's screen differences, which a pixel from the ridge where two
    /// edges meet can point either way: there a sample moves out from `d ≥ narrowest / 2 − 1`.
    /// Rounded up a pixel for the bilinear read, and never past [`Self::reach`]: a pane wider than
    /// its bezel and lean together reads only itself, and a pixel past it.
    #[expect(
        clippy::cast_sign_loss,
        reason = "the overshoot is clamped at 0, and a NaN casts to 0"
    )]
    pub(crate) fn margin(self, index: f32, narrowest: f32) -> u32 {
        let depth = self.bezel.min(0.5 * narrowest);
        let overshoot = (depth + self.lean(index) - narrowest + 1.0).max(0.0);
        (overshoot.ceil() as u32)
            .saturating_add(1)
            .min(self.reach(index))
    }

    /// The tangent bound of [`Self::reach`], unrounded: `thickness · √(n² − 1)`.
    fn lean(self, index: f32) -> f32 {
        self.thickness * (index * index - 1.0).max(0.0).sqrt()
    }
}

/// The closed form the composite shades with, in `f32` as the shader runs it: what the tests
/// check against Snell's law by angles and against the reach.
#[cfg(test)]
mod internals {
    use crate::primitives::math::libm_ext::Libm;
    use crate::shape::paint::lens_optics::LensOptics;

    impl LensOptics {
        /// The glass's height `inward` px in from the edge: the squircle, flat at `thickness` past
        /// the bezel.
        pub(crate) fn height(self, inward: f32) -> f32 {
            let x = (inward / self.bezel).clamp(0.0, 1.0);
            self.thickness * (1.0 - (1.0 - x).libm_powi(4)).libm_powf(0.25)
        }

        /// The height's slope against the inward distance: `thickness / bezel` times the squircle's
        /// derivative `(1 − x)³ · (1 − (1 − x)⁴)^(−¾)`, vertical at the edge and zero past the bezel.
        pub(crate) fn slope(self, inward: f32) -> f32 {
            let x = (inward / self.bezel).clamp(0.0, 1.0);
            if x >= 1.0 {
                return 0.0;
            }
            let g = 1.0 - x;
            let rise = (1.0 - g.libm_powi(4)).max(f32::MIN_POSITIVE);
            self.thickness / self.bezel * g.libm_powi(3) * rise.libm_powf(-0.75)
        }

        /// How far, in px, the backdrop the glass shows `inward` px in from the edge lies further in,
        /// for light of index `index`: the refracted ray, by Snell's law in vector form, carried from
        /// the glass's top down to the plane under it.
        pub(crate) fn displacement(self, inward: f32, index: f32) -> f32 {
            let slope = self.slope(inward);
            // The outward-tilted normal, (slope, 1) normalized, against light falling along −z.
            let norm = slope.libm_hypot(1.0);
            let (nx, nz) = (slope / norm, 1.0 / norm);
            let eta = 1.0 / index;
            let cos_in = nz;
            let k = 1.0 - eta * eta * (1.0 - cos_in * cos_in);
            let along = eta * cos_in - k.sqrt();
            let (tx, tz) = (along * nx, -eta + along * nz);
            // `tx` points outward when negative is inward: the sample moves in by its size.
            self.height(inward) * -tx / -tz
        }
    }
}

#[cfg(test)]
mod tests;
