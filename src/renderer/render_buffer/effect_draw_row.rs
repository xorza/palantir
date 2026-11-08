//! Effect draw records.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::common::span::Span;
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

/// What draws an effect row: an app's loaded effect, or palantir's own backdrop effect for a
/// backdrop filter's pane, in the form its lens takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EffectProgram {
    App(EffectId),
    Pane(PaneForm),
}

/// The pipeline a backdrop filter's pane draws through (`effect_pipeline/pane.wgsl`'s
/// `PANE_FORM`): frost, glass of one index, or glass that spreads the spectrum, each apart as the
/// lens and its loop hold registers, which V3D spilled on every pane while one shader drew them
/// all, and a blurred pane's composite took the Pi 2.2 times as long.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneForm {
    Frost = 0,
    Lens = 1,
    Spectrum = 2,
}

impl PaneForm {
    pub(crate) const COUNT: usize = 3;
    pub(crate) const ALL: [Self; Self::COUNT] = [Self::Frost, Self::Lens, Self::Spectrum];

    pub(crate) const fn idx(self) -> usize {
        self as usize
    }
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
    /// A backdrop effect's row in the frame's panes, the slot its result reads; 0 for a color
    /// effect, whose stages read none.
    pub(crate) pane: u32,
    /// [`Self::WINDOW`] for a pane that fills outside its rounded rect; 0 for every other.
    pub(crate) flags: u32,
}

impl EffectInstance {
    /// `flags`'s bit for a window, a backdrop filter's pane on a windowed rect.
    pub(crate) const WINDOW: u32 = 1;
}
