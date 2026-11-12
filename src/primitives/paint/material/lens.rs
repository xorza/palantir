//! [`Lens`]: glass that bends what a material shows across a rounded bezel at its edge.

use crate::primitives::math::domain;

/// Glass over a material, refracting what it shows across a rounded bezel at its edge, as
/// [`EffectName::GLASS`](crate::EffectName::GLASS) reads it. The bezel rises as a squircle
/// `(1 − (1 − x)⁴)^¼` of the inward fraction `x`; a ray through it bends by Snell's law, and a
/// fixed top-left lamp glints off it.
///
/// Plain data: `bezel` and `thickness` are [lengths](crate::widget::domain::length),
/// `refractive_index` a [refractive index](crate::widget::domain::refractive_index), and
/// `dispersion` and `highlight` [fractions](crate::widget::domain::fraction), each coerced into its
/// kind where the material's reach reads it, as the glass reads its parameters as they come.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[must_use]
pub struct Lens {
    /// The width of the rounded band at the edge, in logical px.
    pub bezel: f32,
    /// The glass's height inside the bezel, in logical px: how far it bends a ray aside.
    pub thickness: f32,
    /// The glass's index of refraction; 1 bends nothing, 1.5 is window glass.
    pub refractive_index: f32,
    /// The index's step from red to blue, `n_F − n_C` between Fraunhofer's C (656.3 nm) and F
    /// (486.1 nm) lines, as a glass catalog gives it; `refractive_index` is the index at the D
    /// line (589.3 nm). It spreads the spectrum across the bezel, fringing what the glass bends
    /// with color. Window glass (N-BK7) has 0.008, dense flint (N-SF11) 0.031; a UI's glass
    /// takes more, so the fringe shows.
    pub dispersion: f32,
    /// The strength of the lamp's glint off the bezel.
    pub highlight: f32,
}

impl Lens {
    /// Glass that bends nothing: no bezel, no thickness, index 1, no fringe and no glint.
    pub const NONE: Self = Self {
        bezel: 0.0,
        thickness: 0.0,
        refractive_index: 1.0,
        dispersion: 0.0,
        highlight: 0.0,
    };

    /// A pill of window glass: a 24 px bezel, 16 px thick, of index 1.5, with a thin rainbow at
    /// the rim and a soft glint.
    pub const WINDOW_GLASS: Self = Self {
        bezel: 24.0,
        thickness: 16.0,
        refractive_index: 1.5,
        dispersion: 0.06,
        highlight: 0.6,
    };

    /// Coerce the bezel and thickness to lengths, the index to at least 1 and the dispersion and
    /// highlight into `0..=1`.
    pub(crate) const fn coerce(&mut self) {
        self.bezel = domain::raised_to(self.bezel, 0.0);
        self.thickness = domain::raised_to(self.thickness, 0.0);
        self.refractive_index = domain::raised_to(self.refractive_index, 1.0);
        self.dispersion = domain::fraction(self.dispersion);
        self.highlight = domain::fraction(self.highlight);
    }
}
