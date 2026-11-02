//! A backdrop lowered to two four-lane words and its luminance ceiling and floor.

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
/// 65504 for a pane with none; `min_luminance` the floor's, rounded up so it never shows darker,
/// and 0 for none. Only a filter that changes the pixels lowers, so a record holds no backdrop as
/// `None`, never as a value of this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoweredBackdrop {
    look: F16x4,
    lens: F16x4,
    max_luminance: u16,
    min_luminance: u16,
}

impl LoweredBackdrop {
    /// `backdrop` as a record keeps it, `None` for a filter that leaves the pixels as they are,
    /// which [`BackdropFilter::is_noop`] decides. [`BackdropFilter::validate`] holds an authored
    /// filter within its kinds, and an animated chrome's tween may carry it past them, as
    /// [`Easing::OutBack`](crate::Easing::OutBack) does past an end: here, where it starts to
    /// paint, each value coerces into its kind, a length or factor up to 0, the index up to 1 and
    /// the lens's fractions into `0..=1`, and saturates at the lanes.
    pub(crate) fn of(backdrop: BackdropFilter) -> Option<Self> {
        if backdrop.is_noop() {
            return None;
        }
        let lens = backdrop.lens.map_or(F16x4::ZERO, |lens| {
            F16x4::from_lanes_saturating([
                lens.bezel.max(0.0),
                lens.thickness.max(0.0),
                lens.refractive_index.max(1.0),
                domain::fraction(lens.dispersion),
            ])
        });
        let highlight = backdrop
            .lens
            .map_or(0.0, |lens| domain::fraction(lens.highlight));
        Some(Self {
            look: F16x4::from_lanes_saturating([
                backdrop.blur.max(0.0),
                backdrop.saturation.max(0.0),
                backdrop.noise.max(0.0),
                highlight,
            ]),
            lens,
            max_luminance: backdrop
                .max_luminance
                .map_or(half::f16::MAX.to_bits(), |v| f16_at_or_below(v.max(0.0))),
            min_luminance: backdrop
                .min_luminance
                .map_or(0, |v| f16_at_or_above(v.max(0.0))),
        })
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

    /// The lowest relative luminance the pane shows: its floor, or 0 with none.
    pub(crate) fn min_luminance(self) -> f32 {
        half::f16::from_bits(self.min_luminance).to_f32()
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

    /// How far past the pane, in physical px, the lens reads under `scale` physical px per
    /// logical px, at its densest color: [`LensOptics::margin`] where `narrowest` gives the
    /// pane's narrowest side in physical px, else [`LensOptics::reach`]. Zero with no lens.
    pub(crate) fn lens_margin(self, scale: f32, narrowest: Option<f32>) -> u32 {
        self.lens().map_or(0, |lens| {
            let optics = LensOptics {
                bezel: lens.bezel * scale,
                thickness: lens.thickness * scale,
                index: lens.refractive_index,
            };
            let index = LensOptics::violet_index(lens.refractive_index, lens.dispersion);
            narrowest.map_or_else(|| optics.reach(index), |n| optics.margin(index, n))
        })
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

/// The bits of the f16 nearest `v` at or above it, `v` a factor saturated at 65504: a floor that
/// rounded down would let a pane show darker than its author derived.
fn f16_at_or_above(v: f32) -> u16 {
    debug_assert!(v >= 0.0, "a floor is a factor: {v}");
    let v = F16x4::saturate_lane(v);
    let nearest = half::f16::from_f32(v);
    if nearest.to_f32() < v {
        nearest.to_bits() + 1
    } else {
        nearest.to_bits()
    }
}

impl hash::Hash for LoweredBackdrop {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.look.as_u64());
        state.write_u64(self.lens.as_u64());
        state.write_u16(self.max_luminance);
        state.write_u16(self.min_luminance);
    }
}

impl NanCheck for LoweredBackdrop {
    fn has_nan(&self) -> bool {
        self.look.has_nan()
            || self.lens.has_nan()
            || half::f16::from_bits(self.max_luminance).is_nan()
            || half::f16::from_bits(self.min_luminance).is_nan()
    }
}
