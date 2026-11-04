//! Sparse side-table indices for optional per-node data.

use crate::common::non_max::NonMaxU32;

/// One node's slots in the three optional side tables; `None` keeps them sparse (a plain leaf pays
/// twelve bytes of zeros).
///
/// # Ceiling
///
/// 32-bit, as a node id is: a table holds at most a row a node, so it fills no sooner than the
/// plane's node ids do. Refused: 16-bit indices, half this column, which a non-virtualized grid of
/// about 300 × 250 cells, each with a grid cell, overran with a panic.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Option<NonMaxU32>,
    pub(crate) panel: Option<NonMaxU32>,
    pub(crate) chrome: Option<NonMaxU32>,
}

unsafe impl bytemuck::NoUninit for ExtrasIdx {}

const _: () = assert!(size_of::<ExtrasIdx>() == 3 * size_of::<u32>());
