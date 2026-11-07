use crate::primitives::math::libm_ext::Libm;
use crate::shape::paint::lens_optics::SPECTRUM_VIOLET;
use crate::shape::paint::lens_optics::{LINE_C, LINE_D, LINE_F, LensOptics, cauchy_offset};

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
            let theta_in = f64::from(LENS.slope(inward)).libm_atan();
            let theta_out = (theta_in.libm_sin() / f64::from(index)).libm_asin();
            let traced = f64::from(LENS.height(inward)) * (theta_in - theta_out).libm_tan();
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
