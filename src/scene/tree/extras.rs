//! Sparse side-table indices for optional per-node data.

use crate::common::index16::Index16;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Option<Index16>,
    pub(crate) panel: Option<Index16>,
    pub(crate) chrome: Option<Index16>,
}
