//! Row index into the gradient row texture. In primitives so the shape store, record store and renderer all depend *down* on one definition (as [`FillKind`](crate::primitives::packed::fill_kind::FillKind)); the texture itself is a renderer resource ([`crate::renderer::row_table`]).

use bytemuck::{Pod, Zeroable};

/// Index into the gradient rows. `LutRow(0)` is the magenta debug fallback so a stray default paints wrong visibly; real rows are `1..capacity`, growing on demand. A newtype so it can't swap with another `u32` on `QuadInstance`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Pod, Zeroable)]
pub(crate) struct LutRow(pub(crate) u32);

impl LutRow {
    /// Sentinel for solid quads, which never sample the LUT; a stray one paints magenta.
    pub(crate) const FALLBACK: Self = Self(0);
}

impl Default for LutRow {
    fn default() -> Self {
        Self::FALLBACK
    }
}
