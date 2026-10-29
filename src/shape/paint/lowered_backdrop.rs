//! A backdrop lowered to two four-lane words and a luminance ceiling.

use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::backdrop_filter::{BackdropFilter, Lens};
use crate::shape::paint::lens_optics::LensOptics;
use std::hash;

/// A [`BackdropFilter`] as a record keeps it: `look = (σ, saturation, noise, highlight)` and
/// `lens = (bezel, thickness, refractive_index, dispersion)`, lengths in logical px, which the
/// composer scales to physical; a pane with no lens has a zero `lens`. `max_luminance` is the
/// ceiling's f16 bits, rounded down so the pane never shows brighter than its author allowed, and
/// 65504 for a pane with none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LoweredBackdrop {
    look: F16x4,
    lens: F16x4,
    max_luminance: u16,
}

impl LoweredBackdrop {
    /// No pane: [`BackdropFilter::NONE`], which leaves the pixels as they are.
    pub(crate) const NONE: Self = Self {
        look: F16x4::ZERO,
        lens: F16x4::ZERO,
        max_luminance: 0,
    };

    /// Whether a rect with this backdrop draws as an ordinary rect.
    pub(crate) const fn is_none(self) -> bool {
        self.look.as_u64() == 0 && self.lens.as_u64() == 0
    }

    /// The blur's σ in logical px.
    pub(crate) fn sigma(self) -> f32 {
        self.look.lanes()[0]
    }

    /// The pane quad's axis lanes, `(0, saturation, noise, opacity)`. The σ stays out: the
    /// composer plans the blur from [`Self::sigma`] in f32, and no shader reads it.
    pub(crate) fn fill_axis(self, opacity: f32) -> FillAxis {
        let [_, saturation, noise, _] = self.look.lanes();
        FillAxis::from_lanes([0.0, saturation, noise, opacity])
    }

    /// The highest relative luminance the pane shows: its ceiling, or 65504 with none.
    pub(crate) fn max_luminance(self) -> f32 {
        half::f16::from_bits(self.max_luminance).to_f32()
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
            max_luminance: backdrop
                .max_luminance
                .map_or(half::f16::MAX.to_bits(), f16_at_or_below),
        }
    }
}

/// The bits of the f16 nearest `v` at or below it, `v` a factor saturated at 65504: a ceiling
/// that rounded up would let a pane show brighter than its author derived.
fn f16_at_or_below(v: f32) -> u16 {
    debug_assert!(v >= 0.0, "a ceiling is a factor: {v}");
    let v = F16x4::saturate_lane(v);
    let nearest = half::f16::from_f32(v);
    if nearest.to_f32() > v {
        nearest.to_bits() - 1
    } else {
        nearest.to_bits()
    }
}

impl hash::Hash for LoweredBackdrop {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.look.as_u64());
        state.write_u64(self.lens.as_u64());
        state.write_u16(self.max_luminance);
    }
}

impl NanCheck for LoweredBackdrop {
    fn has_nan(&self) -> bool {
        self.look.has_nan()
            || self.lens.has_nan()
            || half::f16::from_bits(self.max_luminance).is_nan()
    }
}
