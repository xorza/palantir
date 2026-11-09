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
/// `program` picks the pipeline, and `images` its images in the record store's image column.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct EffectDrawRow {
    pub(crate) program: EffectProgram,
    pub(crate) images: Span,
    pub(crate) instance: EffectInstance,
}

/// What draws an effect row: a loaded effect's shape stages, or its material stages for a
/// material's box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EffectProgram {
    App(EffectId),
    Material(EffectId),
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
    pub(crate) pane: u32,
    /// [`Self::WINDOW`] for a material's box that fills outside its rounded rect; 0 for every
    /// other.
    pub(crate) flags: u32,
}

impl EffectInstance {
    /// `flags`'s bit for a window, a material's box on a windowed rect.
    pub(crate) const WINDOW: u32 = contract::WINDOW_FLAG;
}
