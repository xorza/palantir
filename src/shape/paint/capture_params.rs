//! [`CaptureParams`]: what a backdrop effect's capture is planned from, a material's and an
//! `EffectPaint`'s alike.

use std::hash;

use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::material::backdrop_reach::BackdropReach;
use crate::primitives::paint::material::lens_optics::LensOptics;

/// What the cascade plans a backdrop effect's capture from, in logical px, a material's and an
/// `EffectPaint`'s alike: the σ of its blur and how far past its shape its composite reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CaptureParams {
    pub(crate) sigma: f32,
    pub(crate) reach: CaptureReach,
}

/// How far past its shape a backdrop effect reads: a reach it declares, or a lens, which reads
/// past the shape by what its bend and the shape's size give.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CaptureReach {
    /// A fixed reach in logical px, as an effect's source declares it
    /// ([`EffectSource::reach`](crate::EffectSource::reach)).
    Px(f32),
    /// A material's lens, `(bezel, thickness, refractive_index, dispersion)` in logical px, as
    /// its lane packs it; no bezel reads nothing past the shape.
    Lens(F16x4),
}

impl CaptureReach {
    /// What `reach`, an effect source's, plans a capture from: a lens coerced into its domain and
    /// packed to its lanes.
    ///
    /// # Panics
    ///
    /// Panics unless a fixed reach is a [length](crate::widget::domain::length): the source's
    /// `reach` is app code, and a wrong value would draw quietly wrong.
    pub(crate) fn of(reach: BackdropReach) -> Self {
        match reach {
            BackdropReach::Px(px) => Self::Px(domain::length(px)),
            BackdropReach::Lens(mut lens) => {
                lens.coerce();
                Self::Lens(F16x4::from_lanes_saturating([
                    lens.bezel,
                    lens.thickness,
                    lens.refractive_index,
                    lens.dispersion,
                ]))
            }
        }
    }

    /// How far past the shape, in whole physical px under `scale` physical px per logical px, the
    /// composite reads: a fixed reach rounded out, or a lens's
    /// [`LensOptics::margin`] where `narrowest` gives the shape's narrowest side in physical px,
    /// else [`LensOptics::reach`], at its densest color; zero with no bezel.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a reach is a length, which `Self::of` checks, and a scale is positive"
    )]
    pub(crate) fn margin(self, scale: f32, narrowest: Option<f32>) -> u32 {
        match self {
            Self::Px(px) => (px * scale).ceil() as u32,
            Self::Lens(lens) => {
                let [bezel, thickness, refractive_index, dispersion] = lens.lanes();
                if bezel == 0.0 {
                    return 0;
                }
                let optics = LensOptics {
                    bezel: bezel * scale,
                    thickness: thickness * scale,
                    index: refractive_index,
                };
                let index = LensOptics::violet_index(refractive_index, dispersion);
                narrowest.map_or_else(|| optics.reach(index), |n| optics.margin(index, n))
            }
        }
    }
}

impl hash::Hash for CaptureParams {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.sigma.to_bits());
        match self.reach {
            CaptureReach::Px(px) => {
                state.write_u8(0);
                state.write_u32(px.to_bits());
            }
            CaptureReach::Lens(lens) => {
                state.write_u8(1);
                state.write_u64(lens.as_u64());
            }
        }
    }
}

impl NanCheck for CaptureParams {
    fn has_nan(&self) -> bool {
        self.sigma.is_nan()
            || match self.reach {
                CaptureReach::Px(px) => px.is_nan(),
                CaptureReach::Lens(lens) => lens.has_nan(),
            }
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

    use crate::internals::panic_probe;
    use crate::primitives::math::domain;
    use crate::primitives::packed::half_simd::F16x4;
    use crate::primitives::paint::material::backdrop_reach::BackdropReach;
    use crate::primitives::paint::material::lens::Lens;
    use crate::primitives::paint::material::lens_optics::LensOptics;
    use crate::shape::paint::capture_params::{CaptureParams, CaptureReach};

    /// A source's reach plans the capture as it gives it: a fixed one as is, a lens packed to its
    /// four lanes, `(bezel, thickness, refractive index, dispersion)`; a fixed reach that is no
    /// length is the source's logic error and panics, whichever paint asks.
    #[test]
    fn a_sources_reach_plans_the_capture() {
        assert_eq!(
            CaptureReach::of(BackdropReach::Px(6.0)),
            CaptureReach::Px(6.0)
        );
        assert_eq!(CaptureReach::of(BackdropReach::NONE), CaptureReach::Px(0.0));
        let lens = Lens {
            bezel: 16.0,
            thickness: 24.0,
            refractive_index: 1.5,
            dispersion: 0.25,
            ..Lens::WINDOW_GLASS
        };
        assert_eq!(
            CaptureReach::of(BackdropReach::Lens(lens)),
            CaptureReach::Lens(F16x4::from_lanes([16.0, 24.0, 1.5, 0.25])),
        );
        for bad in [-1.0, f32::NAN, f32::INFINITY] {
            panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
                CaptureReach::of(BackdropReach::Px(bad))
            });
        }
    }

    /// A fixed reach rounds out to whole physical px whatever the shape's side: 2.3 logical px at
    /// scale 2 is 4.6, so 5, and 2.5 is exactly 5. A lens with no bezel reads nothing past the
    /// shape; one with a bezel reads what its optics give, at the shape's narrowest side where it is
    /// known.
    #[test]
    fn a_capture_reads_past_its_shape_by_its_reach_or_its_lens() {
        assert_eq!(CaptureReach::Px(2.3).margin(2.0, None), 5);
        assert_eq!(CaptureReach::Px(2.5).margin(2.0, Some(1.0)), 5);
        assert_eq!(CaptureReach::Px(0.0).margin(2.0, None), 0);
        let lens = |bezel| CaptureReach::Lens(F16x4::from_lanes([bezel, 12.0, 1.5, 0.0]));
        assert_eq!(lens(0.0).margin(2.0, Some(40.0)), 0);
        let optics = LensOptics {
            bezel: 16.0,
            thickness: 24.0,
            index: 1.5,
        };
        let index = LensOptics::violet_index(1.5, 0.0);
        assert_eq!(lens(8.0).margin(2.0, None), optics.reach(index));
        assert_eq!(
            lens(8.0).margin(2.0, Some(40.0)),
            optics.margin(index, 40.0)
        );
        assert_ne!(optics.reach(index), 0, "the lens reads past its shape");
    }

    /// A capture hashes apart by each thing it reads: the σ, the reach, and a fixed reach from a
    /// lens, both of all-zero bits.
    #[test]
    fn a_capture_hashes_by_what_it_reads() {
        let hash = |capture: CaptureParams| {
            BuildHasherDefault::<DefaultHasher>::default().hash_one(capture)
        };
        let capture = |sigma, reach| CaptureParams {
            sigma,
            reach: CaptureReach::Px(reach),
        };
        let base = hash(capture(4.0, 2.0));
        assert_eq!(base, hash(capture(4.0, 2.0)));
        assert_ne!(base, hash(capture(5.0, 2.0)));
        assert_ne!(base, hash(capture(4.0, 3.0)));
        let zero = |reach| hash(CaptureParams { sigma: 0.0, reach });
        assert_ne!(
            zero(CaptureReach::Px(0.0)),
            zero(CaptureReach::Lens(F16x4::from_lanes([0.0; 4]))),
        );
    }
}
