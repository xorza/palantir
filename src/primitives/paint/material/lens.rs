//! [`Lens`]: glass that bends what a material shows across a rounded bezel at its edge.

use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;

/// Glass over a material, refracting what it shows across a rounded bezel at its edge, as
/// [`EffectName::GLASS`](crate::EffectName::GLASS) reads it. The bezel rises as a squircle
/// `(1 − (1 − x)⁴)^¼` of the inward fraction `x`; a ray through it bends by Snell's law, and a
/// fixed top-left lamp glints off it.
///
/// Plain data: `bezel` and `thickness` are [lengths](crate::widget::domain::length), and
/// `refractive_index` a [refractive index](crate::widget::domain::refractive_index), each a
/// [lane](crate::widget::domain::lane) as it packs into one.
/// `dispersion` and `highlight` are [fractions](crate::widget::domain::fraction), clamped
/// into `0..=1` where they paint.
#[repr(C)]
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(default = "Lens::window_glass")]
#[must_use]
pub struct Lens {
    /// The width of the rounded band at the edge, in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::lane_length")]
    pub bezel: f32,
    /// The glass's height inside the bezel, in logical px: how far it bends a ray aside.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::lane_length")]
    pub thickness: f32,
    /// The glass's index of refraction; 1 bends nothing, 1.5 is window glass.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::lane_refractive_index")]
    pub refractive_index: f32,
    /// The index's step from red to blue, `n_F − n_C` between Fraunhofer's C (656.3 nm) and F
    /// (486.1 nm) lines, as a glass catalog gives it; `refractive_index` is the index at the D
    /// line (589.3 nm). It spreads the spectrum across the bezel, fringing what the glass bends
    /// with color. Window glass (N-BK7) has 0.008, dense flint (N-SF11) 0.031; a UI's glass
    /// takes more, so the fringe shows.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::fraction")]
    pub dispersion: f32,
    /// The strength of the lamp's glint off the bezel.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::fraction")]
    pub highlight: f32,
}

impl Lens {
    /// Glass that bends nothing: no bezel, no thickness, index 1, no fringe and no glint. An
    /// absent material blends through it.
    pub const NONE: Self = Self {
        bezel: 0.0,
        thickness: 0.0,
        refractive_index: 1.0,
        dispersion: 0.0,
        highlight: 0.0,
    };

    /// A pill of window glass: a 24 px bezel, 16 px thick, of index 1.5, with a thin rainbow at
    /// the rim and a soft glint. What a theme file's lens holds where it leaves a field out.
    pub const WINDOW_GLASS: Self = Self {
        bezel: 24.0,
        thickness: 16.0,
        refractive_index: 1.5,
        dispersion: 0.06,
        highlight: 0.6,
    };

    /// [`Self::WINDOW_GLASS`], as serde's container default names a function.
    const fn window_glass() -> Self {
        Self::WINDOW_GLASS
    }

    /// Coerce the bezel and thickness to lengths, the index to at least 1 and the dispersion and
    /// highlight into `0..=1`.
    pub(crate) const fn coerce(&mut self) {
        self.bezel = domain::raised_to(self.bezel, 0.0);
        self.thickness = domain::raised_to(self.thickness, 0.0);
        self.refractive_index = domain::raised_to(self.refractive_index, 1.0);
        self.dispersion = domain::fraction(self.dispersion);
        self.highlight = domain::fraction(self.highlight);
    }

    /// Whether `bezel` and `thickness` are lengths and `refractive_index` a refractive index, each
    /// a lane: [`Self::validate`]'s test.
    #[inline]
    pub(crate) const fn is_valid(&self) -> bool {
        domain::are_length_lanes([self.bezel, self.thickness])
            & domain::is_refractive_index(self.refractive_index)
            & domain::is_lane(self.refractive_index)
    }

    /// Panics unless `bezel` and `thickness` are lengths and `refractive_index` a refractive
    /// index, each a [lane](domain::lane) as it packs into one.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        domain::lane(domain::length(self.bezel));
        domain::lane(domain::length(self.thickness));
        domain::lane(domain::refractive_index(self.refractive_index));
    }

    pub(crate) fn has_nan(self) -> bool {
        [
            self.bezel,
            self.thickness,
            self.refractive_index,
            self.dispersion,
            self.highlight,
        ]
        .iter()
        .any(|v| v.is_nan())
    }
}

/// Field by field, as `f32` blends.
impl Lerp for Lens {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        Self {
            bezel: f32::lerp(&a.bezel, &b.bezel, t),
            thickness: f32::lerp(&a.thickness, &b.thickness, t),
            refractive_index: f32::lerp(&a.refractive_index, &b.refractive_index, t),
            dispersion: f32::lerp(&a.dispersion, &b.dispersion, t),
            highlight: f32::lerp(&a.highlight, &b.highlight, t),
        }
    }
}
