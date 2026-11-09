//! Effect draw records.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::common::span::Span;
use crate::effect::contract;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
use glam::Vec2;
use soa_rs::Soars;

/// One effect draw row. SoA storage lets the backend upload `rows.instance()` in one write;
/// `effect` picks the pipeline, and `images` its images in the record store's image column; the
/// instance's flags tell a material's box, which the effect's one stage draws through palantir's
/// material path.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct EffectDrawRow {
    pub(crate) effect: EffectId,
    pub(crate) images: Span,
    pub(crate) instance: EffectInstance,
}

/// Per-shape GPU state an effect's stages read (`effect/wrapper.wgsl`'s `PalantirInstance`).
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EffectInstance {
    /// Physical-px effect rect, before the antialiasing reach.
    pub(crate) rect: Rect,
    /// Physical-px corner radii, fitted to `rect`: top-left, top-right, bottom-right, bottom-left.
    pub(crate) corners: [f32; 4],
    /// Seconds into the shape's paint animation's pass.
    pub(crate) time: f32,
    /// What the coverage is multiplied by: a paint animation's fade, 1 when still.
    pub(crate) opacity: f32,
    /// Physical px per logical px.
    pub(crate) scale: f32,
    /// A paint animation's turn about `rect`'s center, in radians; 0 for none.
    pub(crate) rotation: f32,
    /// The word the instance's parameters start at in the window's parameter buffer.
    pub(crate) params: u32,
    /// A backdrop effect's row in the frame's backdrops, the slot its result reads; 0 for a color
    /// effect, whose stages read none.
    pub(crate) backdrop_idx: u32,
    /// [`Self::MATERIAL`] for a material's box, with [`Self::OUTSIDE`] where it covers outside its
    /// rounded rect or [`Self::TRIANGLE`] for a triangle's; 0 for an app's draw.
    pub(crate) flags: u32,
    /// The shape as the shape template takes it, as `Quad::points` holds a box's: the corners of
    /// `rect` relative to its min, clockwise.
    pub(crate) points: [Vec2; 4],
    /// How deep the shape's interior reaches (`Quad::deep`).
    pub(crate) deep: f32,
    /// A material's border width, physical px, which the template keeps out of its interior; 0
    /// for an app's draw.
    pub(crate) border_width: f32,
}

impl EffectInstance {
    /// `flags`'s bit for a material's box that covers outside its rounded rect.
    pub(crate) const OUTSIDE: u32 = contract::OUTSIDE_FLAG;
    /// `flags`'s bit for a material's box.
    pub(crate) const MATERIAL: u32 = contract::MATERIAL_FLAG;
    /// `flags`'s bit for a triangle, three of `points`.
    pub(crate) const TRIANGLE: u32 = contract::TRIANGLE_FLAG;

    /// The corners of the shape the template draws: 3 for a triangle, 4 for a rect.
    pub(crate) const fn corner_count(&self) -> u32 {
        if self.flags & Self::TRIANGLE != 0 {
            3
        } else {
            4
        }
    }
}
