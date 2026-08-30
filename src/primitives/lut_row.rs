//! Row index into the gradient LUT atlas texture.
//!
//! At the primitives layer for the same reason as
//! [`FillKind`](crate::primitives::fill_kind::FillKind): the shape
//! store, the record store, and the renderer all name the atlas row,
//! and all three depend *down* on this one definition. The LUT texture
//! it indexes is a renderer resource
//! ([`crate::renderer::gradient_atlas`]).

use bytemuck::{Pod, Zeroable};

/// Index into the gradient LUT atlas texture. `LutRow(0)` is the
/// magenta debug fallback (so a stray default value paints obviously
/// wrong); real registrations occupy `1..capacity`, where the atlas
/// grows on demand rather than holding a fixed row count. Newtype keeps
/// the atlas-row identifier from being silently swapped with another
/// `u32` field on `Quad`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct LutRow(pub(crate) u32);

impl LutRow {
    /// Sentinel for solid (non-gradient) quads. The shader only samples
    /// the LUT when `fill_kind` is a gradient, so the value is unused
    /// in that path; a stray `FALLBACK` reaching the sampler paints
    /// magenta.
    pub(crate) const FALLBACK: LutRow = LutRow(0);
}

/// Written out rather than derived, so row 0 is spelled once: the
/// default *is* the fallback, which is what makes an unset row paint
/// magenta instead of somebody's gradient.
impl Default for LutRow {
    fn default() -> Self {
        Self::FALLBACK
    }
}
