//! The path light takes through a pane's lens: the closed form the composite shades with.

use crate::shape::paint::spectrum_tables::SPECTRUM_VIOLET;

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
    use crate::shape::paint::lens_optics::LensOptics;

    impl LensOptics {
        /// The glass's height `inward` px in from the edge: the squircle, flat at `thickness` past
        /// the bezel.
        pub(crate) fn height(self, inward: f32) -> f32 {
            let x = (inward / self.bezel).clamp(0.0, 1.0);
            self.thickness * (1.0 - (1.0 - x).powi(4)).powf(0.25)
        }

        /// The height's slope against the inward distance: `thickness / bezel` times the squircle's
        /// derivative `(1 − x)³ · (1 − (1 − x)⁴)^(−¾)`, vertical at the edge and zero past the bezel.
        pub(crate) fn slope(self, inward: f32) -> f32 {
            let x = (inward / self.bezel).clamp(0.0, 1.0);
            if x >= 1.0 {
                return 0.0;
            }
            let g = 1.0 - x;
            let rise = (1.0 - g.powi(4)).max(f32::MIN_POSITIVE);
            self.thickness / self.bezel * g.powi(3) * rise.powf(-0.75)
        }

        /// How far, in px, the backdrop the glass shows `inward` px in from the edge lies further in,
        /// for light of index `index`: the refracted ray, by Snell's law in vector form, carried from
        /// the glass's top down to the plane under it.
        pub(crate) fn displacement(self, inward: f32, index: f32) -> f32 {
            let slope = self.slope(inward);
            // The outward-tilted normal, (slope, 1) normalized, against light falling along −z.
            let norm = slope.hypot(1.0);
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
mod tests {
    use crate::shape::paint::lens_optics::{LINE_C, LINE_D, LINE_F, LensOptics, cauchy_offset};
    use crate::shape::paint::spectrum_tables::SPECTRUM_VIOLET;

    const LENS: LensOptics = LensOptics {
        bezel: 24.0,
        thickness: 16.0,
        index: 1.5,
    };

    /// The vector form agrees with Snell's law traced by angles: the surface tilts by
    /// `θ₁ = atan(slope)`, the ray leaves at `θ₂ = asin(sin θ₁ / n)` from the normal, so it
    /// leans `θ₁ − θ₂` off vertical, toward the center, and covers the glass's height at that
    /// lean. Relative 1e-4, `f32` through a square root and a `tan`.
    #[test]
    fn the_closed_form_matches_snells_law_by_angles() {
        for index in [1.2f32, 1.5, 1.9] {
            for i in 0..=48 {
                let inward = i as f32 * 0.5;
                let theta_in = f64::from(LENS.slope(inward)).atan();
                let theta_out = (theta_in.sin() / f64::from(index)).asin();
                let traced = f64::from(LENS.height(inward)) * (theta_in - theta_out).tan();
                let got = f64::from(LENS.displacement(inward, index));
                assert!(
                    (got - traced).abs() <= 1e-4 * traced.abs().max(1.0),
                    "n {index}, {inward} px in: {got} against {traced}"
                );
            }
        }
    }

    /// Past the bezel the glass is flat and sends nothing aside; at the very edge it has no
    /// height; between, a denser glass bends further; the reach bounds every sample.
    #[test]
    fn the_lens_bends_only_its_bezel_and_reach_bounds_it() {
        assert_eq!(LENS.displacement(24.0, 1.5), 0.0);
        assert_eq!(LENS.displacement(40.0, 1.5), 0.0);
        assert_eq!(LENS.displacement(0.0, 1.5), 0.0);
        let mid = 6.0;
        assert!(LENS.displacement(mid, 1.9) > LENS.displacement(mid, 1.2));
        assert!(LENS.displacement(mid, 1.2) > 0.0);
        let reach = LENS.reach(1.9) as f32;
        for i in 0..=2400 {
            let inward = i as f32 * 0.01;
            assert!(
                LENS.displacement(inward, 1.9).abs() <= reach,
                "{inward} px in"
            );
        }
        // The widest lens at scale 2: 131008 · √(65504² − 1) ≈ 8.6e9 px saturates.
        let widest = LensOptics {
            thickness: 131008.0,
            ..LENS
        };
        assert_eq!(widest.reach(65504.0), u32::MAX);
    }

    /// A sample `d` px in lands `d + displacement` px along the inward normal; a pane whose
    /// narrowest side holds that keeps it, and one narrower lets it out by the difference, and a
    /// pixel for a normal turned at the ridge. The lean at 1.9 is `16 · √(3.61 − 1) = 25.85`, so
    /// its reach is 27. A 64 px pane holds the 24 px bezel, the lean and the pixel, 50.85, and
    /// reads a pixel past itself; a 40 px pane lets a sample out by `20 + 25.85 − 40 + 1 = 6.85`,
    /// so 8; a 4 px pane by `2 + 25.85 − 4 + 1`, so 26. Over the bezel the closed form stays
    /// inside each margin, moved inward, and within a pixel of the ridge moved out.
    #[test]
    fn a_wide_pane_holds_what_its_bezel_bends() {
        assert_eq!(LENS.reach(1.9), 27);
        assert_eq!(LENS.margin(1.9, 64.0), 1);
        assert_eq!(LENS.margin(1.9, 40.0), 8);
        assert_eq!(LENS.margin(1.9, 4.0), 26);
        assert_eq!(LENS.margin(1.0, 4.0), 1);
        for narrowest in [4.0, 12.0, 30.0, 40.0, 48.0, 64.0] {
            let margin = LENS.margin(1.9, narrowest) as f32;
            for i in 0..=1200 {
                let inward = (i as f32 * 0.01).min(0.5 * narrowest);
                let shift = LENS.displacement(inward, 1.9);
                let out = inward + shift - narrowest;
                assert!(
                    out <= margin - 1.0,
                    "{narrowest} wide, {inward} px in: {out}"
                );
                if inward >= 0.5 * narrowest - 1.0 {
                    let turned = shift - inward;
                    assert!(
                        turned <= margin - 1.0,
                        "{narrowest} wide, {inward} px in, turned: {turned}"
                    );
                }
            }
        }
    }

    /// Cauchy's offset is 0 at D and steps by 1 from C to F, so a lens's dispersion is its
    /// `n_F − n_C`; at 400 nm it is `(6.25 − 2.8796) / (4.2321 − 2.3217) ≈ 1.7643` (µm⁻²), so a
    /// lens of index 1.5 and dispersion 0.02 reaches 1.5353 there.
    #[test]
    fn the_index_follows_cauchy_through_d_with_its_dispersion_from_c_to_f() {
        assert_eq!(cauchy_offset(LINE_D), 0.0);
        assert!((cauchy_offset(LINE_F) - cauchy_offset(LINE_C) - 1.0).abs() < 1e-5);
        assert!((cauchy_offset(SPECTRUM_VIOLET) - 1.7643).abs() < 1e-3);
        let violet = LensOptics::violet_index(1.5, 0.02);
        assert!((violet - 1.535_29).abs() < 1e-4, "{violet}");
        assert_eq!(LensOptics::violet_index(1.5, 0.0), 1.5);
    }
}
