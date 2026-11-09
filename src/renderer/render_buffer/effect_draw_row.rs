//! Effect draw records.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::common::span::Span;
use crate::effect::contract;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
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
    /// [`Self::MATERIAL`] for a material's box, with [`Self::WINDOW`] where it covers outside its
    /// rounded rect; 0 for an app's draw.
    pub(crate) flags: u32,
}

impl EffectInstance {
    /// `flags`'s bit for a material's box that covers outside its rounded rect.
    pub(crate) const WINDOW: u32 = contract::WINDOW_FLAG;
    /// `flags`'s bit for a material's box.
    pub(crate) const MATERIAL: u32 = contract::MATERIAL_FLAG;
}
