//! Sparse side-table indices for optional per-node data.

use crate::common::index16::Index16;

/// One node's slots in the three optional side tables; `None` keeps them sparse (a plain leaf pays six bytes).
///
/// # Ceiling
///
/// [`Index16`] caps each table at [`Index16::LAST`]` + 1` rows **per layer, per frame**; a row past that
/// panics in release rather than wrapping. 32-bit indices would double this per-node SoA column.
///
/// A large scene fills `chrome` first (any node with a paintable [`Background`] or rounded clip).
///
/// [`Background`]: crate::Background
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Option<Index16>,
    pub(crate) panel: Option<Index16>,
    pub(crate) chrome: Option<Index16>,
}

// SAFETY: `repr(C)` over three `Option<Index16>`, each two bytes of a
// `NonZeroU16` with `None` at zero, whose sizes sum to the struct's, so
// every byte is initialized and none is padding.
unsafe impl bytemuck::NoUninit for ExtrasIdx {}

const _: () = assert!(size_of::<ExtrasIdx>() == 3 * size_of::<u16>());
