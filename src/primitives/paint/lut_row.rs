//! Row index into the gradient LUT atlas texture. In primitives so the shape store, record store and renderer all depend *down* on one definition (as [`FillKind`](crate::primitives::packed::fill_kind::FillKind)); the texture itself is a renderer resource ([`crate::renderer::gradient_atlas`]).

use bytemuck::{Pod, Zeroable};

/// Index into the gradient LUT atlas. `LutRow(0)` is the magenta debug fallback so a stray default paints wrong visibly; real rows are `1..capacity`, growing on demand. A newtype so it can't swap with another `u32` on `Quad`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct LutRow(pub(crate) u32);

impl LutRow {
    /// Sentinel for solid quads; the shader samples the LUT only for gradient `fill_kind`, and a stray `FALLBACK` paints magenta.
    pub(crate) const FALLBACK: LutRow = LutRow(0);
}

/// Written out so row 0 is spelled once: the default *is* the fallback, so an unset row paints magenta.
impl Default for LutRow {
    fn default() -> Self {
        Self::FALLBACK
    }
}
