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

impl LensOptics {
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
        ((self.thickness * (index * index - 1.0).max(0.0).sqrt()).ceil() as u32).saturating_add(1)
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
    use crate::shape::paint::lens_optics::LensOptics;

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
}
