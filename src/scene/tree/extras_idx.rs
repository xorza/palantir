//! Sparse side-table indices for optional per-node data.

use crate::common::idx16::Idx16;

/// One node's slots in the three optional side tables; `None` keeps them sparse (a plain leaf pays six bytes).
///
/// # Ceiling
///
/// [`Idx16`] caps each table at [`Idx16::LAST`]` + 1` rows **per layer, per frame**; a row past that
/// panics in release rather than wrapping. 32-bit indices would double this per-node SoA column.
///
/// A large scene fills `chrome` first (any node with a paintable [`Bg`] or rounded clip).
///
/// [`Bg`]: crate::Bg
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Option<Idx16>,
    pub(crate) panel: Option<Idx16>,
    pub(crate) chrome: Option<Idx16>,
}

unsafe impl bytemuck::NoUninit for ExtrasIdx {}

const _: () = assert!(size_of::<ExtrasIdx>() == 3 * size_of::<u16>());
