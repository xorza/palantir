//! A backdrop lowered to two four-lane words.

use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::backdrop_filter::{BackdropFilter, Lens};
use crate::shape::paint::lens_optics::LensOptics;
use std::hash;

/// A [`BackdropFilter`] as a record keeps it: `look = (σ, saturation, noise, highlight)` and
/// `lens = (bezel, thickness, refractive_index, dispersion)`, lengths in logical px, which the
/// composer scales to physical; a pane with no lens has a zero `lens`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LoweredBackdrop {
    look: F16x4,
    lens: F16x4,
}

impl LoweredBackdrop {
    /// No pane: [`BackdropFilter::NONE`], which leaves the pixels as they are.
    pub(crate) const NONE: Self = Self {
        look: F16x4::ZERO,
        lens: F16x4::ZERO,
    };

    /// Whether a rect with this backdrop draws as an ordinary rect.
    pub(crate) const fn is_none(self) -> bool {
        self.look.as_u64() == 0 && self.lens.as_u64() == 0
    }

    /// The blur's σ in logical px.
    pub(crate) fn sigma(self) -> f32 {
        self.look.lanes()[0]
    }

    /// The pane quad's axis lanes, `(σ, saturation, noise, opacity)`, at full opacity.
    pub(crate) fn fill_axis(self) -> FillAxis {
        let [sigma, saturation, noise, _] = self.look.lanes();
        FillAxis::from_lanes([sigma, saturation, noise, 1.0])
    }

    /// The lens, `None` for a pane of no glass.
    pub(crate) fn lens(self) -> Option<Lens> {
        if self.lens.as_u64() == 0 {
            return None;
        }
        let [bezel, thickness, refractive_index, dispersion] = self.lens.lanes();
        Some(Lens {
            bezel,
            thickness,
            refractive_index,
            dispersion,
            highlight: self.look.lanes()[3],
        })
    }

    /// How far, in physical px, the lens sends a sample aside under `scale` physical px per
    /// logical px: the reach of its densest color. Zero with no lens.
    pub(crate) fn lens_reach(self, scale: f32) -> u32 {
        self.lens().map_or(0, |lens| {
            LensOptics {
                bezel: lens.bezel * scale,
                thickness: lens.thickness * scale,
                index: lens.refractive_index,
            }
            .reach(lens.refractive_index + 0.5 * lens.dispersion)
        })
    }
}

/// Saturating: [`BackdropFilter::validate`] holds an authored filter within the lanes, and an
/// animated chrome's tween may overshoot it.
impl From<BackdropFilter> for LoweredBackdrop {
    fn from(backdrop: BackdropFilter) -> Self {
        if backdrop.is_noop() {
            return Self::NONE;
        }
        // The lens's fractions coerce here, where they start to paint.
        let lens = backdrop.lens.map_or(F16x4::ZERO, |lens| {
            F16x4::from_lanes_saturating([
                lens.bezel,
                lens.thickness,
                lens.refractive_index,
                domain::fraction(lens.dispersion),
            ])
        });
        let highlight = backdrop
            .lens
            .map_or(0.0, |lens| domain::fraction(lens.highlight));
        Self {
            look: F16x4::from_lanes_saturating([
                backdrop.blur,
                backdrop.saturation,
                backdrop.noise,
                highlight,
            ]),
            lens,
        }
    }
}

impl hash::Hash for LoweredBackdrop {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.look.as_u64());
        state.write_u64(self.lens.as_u64());
    }
}

impl NanCheck for LoweredBackdrop {
    fn has_nan(&self) -> bool {
        self.look.has_nan() || self.lens.has_nan()
    }
}
