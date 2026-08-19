//! Sparse side-table indices for optional per-node data.

use crate::common::index16::Index16;

/// One node's slots in the three optional side tables. `None` means the
/// node carries none of that kind, which is the common case — the tables
/// are sparse so a plain leaf pays six bytes here instead of a full row
/// in each.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Option<Index16>,
    pub(crate) panel: Option<Index16>,
    pub(crate) chrome: Option<Index16>,
}
